//! A missing or rejected server acknowledgment never proves retirement.
use std::time::Duration;

pub(super) fn wait(
    mut poll: impl FnMut() -> Result<Option<bool>, &'static str>,
    mut elapsed: impl FnMut() -> Duration,
    mut idle: impl FnMut(),
    on_stall: &mut impl FnMut(),
) -> Result<(), &'static str> {
    let mut warned = false;
    loop {
        if poll()? == Some(true) {
            return Ok(());
        }
        if !warned && elapsed() >= Duration::from_millis(250) {
            warned = true;
            on_stall();
        }
        idle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn delayed_ack_warns_once_and_never_retires_early() {
        let polls = Cell::new(0);
        let warnings = Cell::new(0);
        wait(
            || {
                polls.set(polls.get() + 1);
                Ok((polls.get() == 10).then_some(true))
            },
            || Duration::from_millis(polls.get() * 100),
            || {},
            &mut || warnings.set(warnings.get() + 1),
        )
        .unwrap();
        assert_eq!(polls.get(), 10);
        assert_eq!(warnings.get(), 1);
    }

    #[test]
    fn rejected_ack_retains_ownership_until_connection_loss() {
        let polls = Cell::new(0);
        let warnings = Cell::new(0);
        let result = wait(
            || {
                polls.set(polls.get() + 1);
                if polls.get() == 8 {
                    Err("OUTPUT_LOST")
                } else {
                    Ok(Some(false))
                }
            },
            || Duration::from_millis(polls.get() * 100),
            || {},
            &mut || warnings.set(warnings.get() + 1),
        );
        assert_eq!(result, Err("OUTPUT_LOST"));
        assert_eq!(polls.get(), 8);
        assert_eq!(warnings.get(), 1);
    }

    #[test]
    fn timely_ack_does_not_warn() {
        let warnings = Cell::new(0);
        wait(|| Ok(Some(true)), || Duration::ZERO, || {}, &mut || {
            warnings.set(1)
        })
        .unwrap();
        assert_eq!(warnings.get(), 0);
    }
}
