use super::*;
use crate::playback::continuity::SuccessorFence;

fn render(
    consumer: &mut BoundaryPcmConsumer,
    a: &ArrayQueue<f32>,
    b: &ArrayQueue<f32>,
    tail: bool,
) -> ([f32; 4], BoundaryRendered) {
    let mut samples = [99.0; 4];
    let result = if tail {
        consumer.render_with_tail(
            &mut samples,
            a,
            true,
            true,
            b,
            true,
            true,
            &ArrayQueue::new(8),
            &SubmittedTail::new(8),
            true,
        )
    } else {
        consumer.render(&mut samples, a, true, true, b, true, true, true)
    };
    (samples, result)
}

#[test]
fn queue_edit_revokes_either_inactive_slot_without_stopping_current_samples() {
    for tail in [false, true] {
        let fence = Arc::new(SuccessorFence::default());
        let a = ArrayQueue::new(8);
        let b = ArrayQueue::new(8);
        let mut consumer = BoundaryPcmConsumer::new(2, 2).with_successor_fence(fence.clone());
        for sample in [1.0, 2.0] {
            a.push(sample).unwrap();
        }
        for sample in [3.0, 4.0] {
            b.push(sample).unwrap();
        }
        let stale_epoch = fence.epoch();
        assert!(fence.authorize(stale_epoch));
        assert!(fence.revoke());
        assert!(
            !fence.authorize(stale_epoch),
            "late readiness cannot undo an accepted edit"
        );
        let (samples, result) = render(&mut consumer, &a, &b, tail);
        assert_eq!(samples, [1.0, 2.0, 0.0, 0.0]);
        assert_eq!(result.boundary_frame, None);
        assert_eq!(b.len(), 2, "revoked PCM must not be consumed");
        while b.pop().is_some() {}
        for sample in [5.0, 6.0, 7.0, 8.0, 9.0, 10.0] {
            b.push(sample).unwrap();
        }
        assert!(fence.authorize(fence.epoch()));
        let (samples, result) = render(&mut consumer, &a, &b, tail);
        assert_eq!(samples, [5.0, 6.0, 7.0, 8.0]);
        assert_eq!(result.active_slot, 1);
        fence.adopted();
        // B is now active; the successor is A. The fence must alternate too.
        for sample in [11.0, 12.0] {
            a.push(sample).unwrap();
        }
        assert!(fence.authorize(fence.epoch()));
        assert!(fence.revoke());
        let (samples, result) = render(&mut consumer, &a, &b, tail);
        assert_eq!(samples, [9.0, 10.0, 0.0, 0.0]);
        assert_eq!(result.boundary_frame, None);
        assert_eq!(a.len(), 2);
        while a.pop().is_some() {}
        for sample in [13.0, 14.0] {
            a.push(sample).unwrap();
        }
        assert!(fence.authorize(fence.epoch()));
        let (samples, result) = render(&mut consumer, &a, &b, tail);
        assert_eq!(samples, [13.0, 14.0, 0.0, 0.0]);
        assert_eq!(result.active_slot, 0);
    }
}

#[test]
fn rendered_boundary_claim_blocks_edits_before_backend_submission_is_recorded() {
    for tail in [false, true] {
        let fence = Arc::new(SuccessorFence::default());
        let a = ArrayQueue::new(4);
        let b = ArrayQueue::new(4);
        b.push(0.25).unwrap();
        b.push(0.5).unwrap();
        let mut consumer = BoundaryPcmConsumer::new(2, 2).with_successor_fence(fence.clone());
        let epoch = fence.epoch();
        assert!(fence.authorize(epoch));
        let (scratch, result) = render(&mut consumer, &a, &b, tail);
        assert_eq!(result.boundary_frame, Some(0));
        assert_eq!(&scratch[..2], &[0.25, 0.5]);
        // The callback/Pulse worker has rendered, but no receipt or write exists
        // yet. Claiming inside the consumer already made admission return busy.
        assert!(!fence.revoke());
        assert_eq!(fence.epoch(), epoch);
        assert!(fence.claimed());
        fence.adopted();
        assert!(fence.revoke());
    }
}

#[test]
fn edit_and_boundary_have_exactly_one_winner() {
    for _ in 0..100 {
        let fence = Arc::new(SuccessorFence::default());
        assert!(fence.authorize(fence.epoch()));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_fence = fence.clone();
        let worker_barrier = barrier.clone();
        let callback = std::thread::spawn(move || {
            worker_barrier.wait();
            worker_fence.claim()
        });
        barrier.wait();
        let edited = fence.revoke();
        let submitted = callback.join().unwrap();
        assert_ne!(edited, submitted);
    }
}
