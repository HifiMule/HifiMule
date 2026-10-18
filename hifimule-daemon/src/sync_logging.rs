//! Low-volume, file-backed lifecycle events. Never attach request/config data here.
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

tokio::task_local! { static RUN: Run; }

#[derive(Clone)]
pub(crate) struct Run {
    id: String,
    next: Arc<AtomicU64>,
    #[cfg(test)]
    events: Arc<std::sync::Mutex<Vec<String>>>,
}
impl Run {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            next: Arc::new(AtomicU64::new(1)),
            #[cfg(test)]
            events: Default::default(),
        }
    }
    pub fn current() -> Self {
        RUN.try_with(Clone::clone).unwrap_or_else(|_| Self::new())
    }
    pub async fn scope<T>(&self, future: impl Future<Output = T>) -> T {
        RUN.scope(self.clone(), future).await
    }
    pub fn link(&self, kind: &'static str, id: &str) {
        // Only internally generated UUIDs are accepted, never arbitrary RPC input.
        if uuid::Uuid::parse_str(id).is_ok() {
            self.emit(format!(
                "[SyncStep] event=link run_id={} {}={}",
                self.id, kind, id
            ));
        }
    }
    fn emit(&self, line: String) {
        crate::daemon_log!("{}", line);
        #[cfg(test)]
        self.events.lock().unwrap().push(line);
    }
    #[cfg(test)]
    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

pub(crate) struct Step {
    run: Run,
    id: u64,
    name: &'static str,
    started: Instant,
    ended: bool,
}
impl Step {
    pub fn current(name: &'static str) -> Self {
        Self::new(Run::current(), name)
    }
    pub fn new(run: Run, name: &'static str) -> Self {
        let id = run.next.fetch_add(1, Ordering::Relaxed);
        run.emit(format!(
            "[SyncStep] event=start run_id={} step_id={} stage={}",
            run.id, id, name
        ));
        Self {
            run,
            id,
            name,
            started: Instant::now(),
            ended: false,
        }
    }
    pub fn finish(&mut self, outcome: &'static str, count: usize) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.run.emit(format!(
            "[SyncStep] event=end run_id={} step_id={} stage={} elapsed_ms={} outcome={} count={}",
            self.run.id,
            self.id,
            self.name,
            self.started.elapsed().as_millis(),
            outcome,
            count
        ));
    }
}
impl Drop for Step {
    fn drop(&mut self) {
        self.finish("interrupted", 0);
    }
}

/// One active bulk phase, kept outside the result-producing async block so a
/// handled early return can terminate it with the actual outcome.
#[derive(Default)]
pub(crate) struct Phases(Option<Step>);
impl Phases {
    pub fn next(&mut self, name: &'static str) {
        self.finish("success", 0);
        self.0 = Some(Step::current(name));
    }
    pub fn finish(&mut self, outcome: &'static str, count: usize) {
        if let Some(mut step) = self.0.take() {
            step.finish(outcome, count);
        }
    }
}
pub(crate) fn outcome(failed: bool, cancelled: bool) -> &'static str {
    if failed {
        "failed"
    } else if cancelled {
        "cancelled"
    } else {
        "success"
    }
}
pub(crate) fn status(status: &crate::sync::SyncStatus) -> &'static str {
    match status {
        crate::sync::SyncStatus::Complete => "success",
        crate::sync::SyncStatus::Failed => "failed",
        crate::sync::SyncStatus::Cancelled => "cancelled",
        _ => "interrupted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn paired_outcomes_and_no_duplicate_terminal_events() {
        let run = Run::new();
        for outcome in ["success", "failed", "cancelled", "skipped", "noop"] {
            let mut step = Step::new(run.clone(), "fixture");
            step.finish(outcome, 3);
            step.finish("success", 0);
        }
        {
            let _dropped = Step::new(run.clone(), "dropped");
        }
        let events = run.events();
        assert_eq!(
            events.iter().filter(|e| e.contains("event=start")).count(),
            6
        );
        assert_eq!(events.iter().filter(|e| e.contains("event=end")).count(), 6);
        assert!(events.last().unwrap().contains("outcome=interrupted"));
        assert!(
            events
                .iter()
                .filter(|e| e.contains("event=end"))
                .all(|e| e.contains("elapsed_ms="))
        );
    }
    #[tokio::test]
    async fn aborted_future_reports_interrupted_once() {
        let run = Run::new();
        let task_run = run.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let task_entered = entered.clone();
        let task = tokio::spawn(async move {
            task_run
                .scope(async {
                    let _step = Step::current("blocked");
                    task_entered.notify_one();
                    std::future::pending::<()>().await;
                })
                .await;
        });
        entered.notified().await;
        assert_eq!(run.events().len(), 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let events = run.events();
        assert_eq!(events.len(), 2);
        assert!(events[1].contains("outcome=interrupted"));
    }

    #[test]
    fn errors_take_precedence_over_cancellation_and_outer_success() {
        assert_eq!(outcome(true, true), "failed");
        assert_eq!(outcome(true, false), "failed");
        assert_eq!(outcome(false, true), "cancelled");
    }
}
