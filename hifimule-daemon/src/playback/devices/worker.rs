//! One owned discovery worker and one serialized preference worker. A timed-out
//! native call remains owned; refreshes coalesce instead of spawning more work.
use super::{Discovery, Labels};
use crate::playback::config::{self, ConfigError, PlaybackConfig};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Inventory {
    pub discovery: Discovery,
    pub sequence: u64,
    pub in_flight_since: Option<Instant>,
}

struct DiscoveryState {
    inventory: Inventory,
    refresh: bool,
    stopping: bool,
}

pub struct DiscoveryWorker {
    timeout: Duration,
    state: Arc<(Mutex<DiscoveryState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl DiscoveryWorker {
    pub fn start(discover: impl Fn() -> Discovery + Send + 'static) -> Self {
        Self::start_with_timeout(discover, Duration::from_secs(5))
    }

    fn start_with_timeout(
        discover: impl Fn() -> Discovery + Send + 'static,
        timeout: Duration,
    ) -> Self {
        let state = Arc::new((
            Mutex::new(DiscoveryState {
                inventory: Inventory {
                    discovery: Discovery {
                        outputs: vec![],
                        error: Some("OUTPUT_DISCOVERY_PENDING"),
                    },
                    sequence: 0,
                    in_flight_since: None,
                },
                refresh: true,
                stopping: false,
            }),
            Condvar::new(),
        ));
        let shared = state.clone();
        let thread = std::thread::Builder::new()
            .name("hifimule-output-discovery".into())
            .spawn(move || {
                let mut labels = Labels::default();
                loop {
                    let (lock, changed) = &*shared;
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    if state.stopping {
                        return;
                    }
                    if !state.refresh {
                        state = changed
                            .wait_timeout(state, Duration::from_secs(1))
                            .unwrap_or_else(|e| e.into_inner())
                            .0;
                    }
                    if state.stopping {
                        return;
                    }
                    state.refresh = false;
                    let started = Instant::now();
                    state.inventory.in_flight_since = Some(started);
                    drop(state);
                    let discovered = labels.reconcile(discover());
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    state.inventory.in_flight_since = None;
                    if state.stopping {
                        return;
                    }
                    state.inventory.discovery = if started.elapsed() > timeout {
                        Discovery {
                            outputs: vec![],
                            error: Some("OUTPUT_DISCOVERY_TIMEOUT"),
                        }
                    } else {
                        discovered
                    };
                    state.inventory.sequence = state.inventory.sequence.wrapping_add(1);
                }
            })
            .expect("output discovery worker must start");
        Self {
            timeout,
            state,
            thread: Some(thread),
        }
    }

    pub fn inventory(&self, refresh: bool) -> Inventory {
        let (lock, changed) = &*self.state;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        if refresh {
            state.refresh = true;
            changed.notify_one();
        }
        let mut result = state.inventory.clone();
        if result
            .in_flight_since
            .is_some_and(|at| at.elapsed() > self.timeout)
        {
            result.discovery.error = Some("OUTPUT_DISCOVERY_TIMEOUT");
            for output in &mut result.discovery.outputs {
                output.available = false;
            }
        }
        result
    }
}

impl Drop for DiscoveryWorker {
    fn drop(&mut self) {
        let (lock, changed) = &*self.state;
        lock.lock().unwrap_or_else(|e| e.into_inner()).stopping = true;
        changed.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone)]
pub struct SaveRequest {
    pub revision: u64,
    pub config: PlaybackConfig,
    pub replace_invalid: bool,
}

#[derive(Clone)]
pub struct SaveResult {
    pub revision: u64,
    pub committed: Result<PlaybackConfig, ConfigError>,
    pub error: Option<ConfigError>,
}

struct SaveState {
    pending: Option<SaveRequest>,
    result: Option<SaveResult>,
    stopping: bool,
}

pub struct PreferenceWorker {
    state: Arc<(Mutex<SaveState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
    latest: Arc<std::sync::atomic::AtomicU64>,
}

impl PreferenceWorker {
    pub fn start(path: PathBuf) -> Self {
        let state = Arc::new((
            Mutex::new(SaveState {
                pending: None,
                result: None,
                stopping: false,
            }),
            Condvar::new(),
        ));
        let shared = state.clone();
        let latest = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let worker_latest = latest.clone();
        let thread = std::thread::Builder::new()
            .name("hifimule-output-preference".into())
            .spawn(move || {
                loop {
                    let (lock, changed) = &*shared;
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while state.pending.is_none() && !state.stopping {
                        state = changed.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    let Some(request) = state.pending.take() else {
                        return;
                    };
                    drop(state);
                    let result = config::save_if_current(
                        &path,
                        &request.config,
                        request.replace_invalid,
                        || {
                            worker_latest.load(std::sync::atomic::Ordering::Acquire)
                                == request.revision
                        },
                    );
                    if result == Ok(false) {
                        continue;
                    }
                    let error = result.err();
                    // Re-read even after a failed fsync: rename may already have
                    // committed. The owner must project the actual durable choice.
                    let committed = config::load(&path);
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    state.result = Some(SaveResult {
                        revision: request.revision,
                        committed,
                        error,
                    });
                }
            })
            .expect("output preference worker must start");
        Self {
            state,
            thread: Some(thread),
            latest,
        }
    }

    pub fn submit(&self, request: SaveRequest) {
        self.latest
            .store(request.revision, std::sync::atomic::Ordering::Release);
        let (lock, changed) = &*self.state;
        lock.lock().unwrap_or_else(|e| e.into_inner()).pending = Some(request);
        changed.notify_one();
    }

    pub fn take_result(&self) -> Option<SaveResult> {
        self.state
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .result
            .take()
    }
}

impl Drop for PreferenceWorker {
    fn drop(&mut self) {
        let (lock, changed) = &*self.state;
        lock.lock().unwrap_or_else(|e| e.into_inner()).stopping = true;
        changed.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    #[test]
    fn discovery_refreshes_coalesce_while_native_worker_remains_owned() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let worker = DiscoveryWorker::start(move || {
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            Discovery {
                outputs: vec![],
                error: None,
            }
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        for _ in 0..1000 {
            worker.inventory(true);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while calls.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        drop(worker);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn timed_out_discovery_retains_worker_and_discards_late_success() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let worker = DiscoveryWorker::start_with_timeout(
            move || {
                observed.fetch_add(1, Ordering::SeqCst);
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Discovery {
                    outputs: vec![],
                    error: None,
                }
            },
            Duration::from_millis(20),
        );
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            worker.inventory(false).discovery.error,
            Some("OUTPUT_DISCOVERY_TIMEOUT")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        release_tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let inventory = worker.inventory(false);
            if inventory.sequence > 0 {
                assert_eq!(inventory.discovery.error, Some("OUTPUT_DISCOVERY_TIMEOUT"));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        drop(worker);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn save_failure_reports_last_actual_commit_and_shutdown_joins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("playback.json");
        config::save(&path, &PlaybackConfig::default(), false).unwrap();
        let worker = PreferenceWorker::start(path.clone());
        worker.submit(SaveRequest {
            revision: 2,
            config: PlaybackConfig {
                schema_version: 2,
                output: None,
            },
            replace_invalid: false,
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let result = loop {
            if let Some(result) = worker.take_result() {
                break result;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(result.revision, 2);
        assert_eq!(result.error, Some(ConfigError::Invalid));
        assert_eq!(result.committed, Ok(PlaybackConfig::default()));
        drop(worker);
        assert_eq!(config::load(&path), Ok(PlaybackConfig::default()));
    }
}
