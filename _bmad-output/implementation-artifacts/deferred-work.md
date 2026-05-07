# Deferred Work

All previously deferred items have been incorporated into Epic 7 stories (7.1–7.4) in `_bmad-output/planning-artifacts/epics.md`.

## Deferred from: code review of 7-2-devicemanager-concurrency-refactor (2026-05-08)

- **TOCTOU in `handle_device_detected`** — read-lock `contains_key` check followed by separate write-lock insert; two concurrent callers can both pass the guard and both insert for the same path. Pre-existing pattern unchanged by this story.
- **MTP tight retry loop on read failure** — `emit_mtp_probe_event` returning `false` leaves the device retryable but the 2-second observer loop has no backoff or retry counter. Intentional per AC4 but needs a broader cooldown design.
- **`list_root_folders` TOCTOU** — selected path can be removed between snapshot lock release and `read_dir`; error propagates via `?`. Pre-existing.
- **`run_observer` silent dropped `Removed` events** — `tx.try_send` for eviction and removal events can silently fail if channel is full, leaving ghost entries in `connected_devices`. Pre-existing mechanism.
- **`get_mounts` accidental volume-disappearance skip** — volumes that disappear between `read_dir` and `is_mount_point` return `false` from `is_mount_point` (not a hard error), so they are not included in `current_mounts`. AC9 is met behaviourally but without explicit handling.
