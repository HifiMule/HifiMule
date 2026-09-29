# Story 16.14 sustained-session evidence contract — version 1

Status: frozen before acceptance runs on 2026-09-29. A threshold change requires a new
contract version, a rationale, and complete reruns. It may not retroactively change a result.

## Runs and sampling

Every package row runs three repetitions of each profile. The short coexistence profile lasts
30 minutes after a 5-minute warm-up. The sustained album and Radio profiles each last 8 hours
after the same warm-up. Idle and active playback-only baselines use the same duration, machine,
package, provider fixture, output, and 5-second sampling interval. Samples use a monotonic clock.

The fixed workload is a 240-track/twenty-artist fixture across Jellyfin and OpenSubsonic, a
10,000-occurrence preloaded history, 200-row history pages, and one representative physical
device sync of at least 20 GiB or the device's remaining managed capacity. Radio checks refill
every transition, keeps no more than five automatic upcoming entries, and applies a deterministic
15-minute cadence: seek, Preview/Return, one manual insertion, one automatic removal, output
switch/loss, and alternating reporting/feedback/export operations. The fault schedule additionally
injects a 15-second source stall at minute 20, a 60-second outage at minute 40, an ambiguous remote
write at minute 55, sleep/wake after hour 2, orderly Quit after hour 4, abrupt exit after hour 6,
and a killed-mid-checkpoint restart in a separate copy of the fixture.

## Frozen gates (native units)

| Owner | Gate |
|---|---|
| Process RSS | Active p95 <= matching idle p95 + 134,217,728 bytes; retained growth over the final 1,200 seconds <= 16,777,216 bytes. |
| Cross-platform process cost | Same-fixture active RSS p95 delta <= 15%; normalized CPU p95 delta <= 15%. Unlike native-heap collectors are never compared as equivalent. |
| Native heap | Record when the platform exposes a stable installed-process collector. Compare sustained active p95 and slope with the matching active baseline; `unavailable` is a limitation, not a pass. |
| Compressed input | <= 8,388,608 bytes/source and <= 16,777,216 bytes across two slots; reader chunk 65,536 bytes; upstream chunk <= 1,048,576 bytes; retained seek window <= 7,274,496 bytes. |
| Decoded PCM | <= 1,048,576 bytes/source and <= 2,097,152 bytes aggregate. |
| Radio/adaptation | <= 5 automatic upcoming entries; refill threshold 2; <= 5 preparation failures; <= 16 samples/scope and <= 64 scopes. |
| History | API page <= 200 rows; restart restore batches <= 200; UI retains at most two requested pages (400 rows). The 10,000-row fixture must have stable order and no omission/duplication. |
| Sync staging | <= 2 tracks and <= 2,147,483,648 bytes. |
| Operations | Each command has one live owner entry; reporting, feedback, playlist, and basket queues have no duplicate command/source identity. Ambiguous writes stay durable and are not automatically retried. |
| Cleanup | Retired request/decoder/output tasks and handles return to the matching active baseline within 15 seconds; orderly Quit completes within 60 seconds. |
| Recovery | Restore the last committed state paused. No output reroute or resume occurs on wake/reconnect. Stale session/generation/occurrence/command completions publish nothing. |
| Coexistence | Healthy combined median sync throughput is within 5% of sync-only with zero protection decisions. Slow-device-only produces zero playback-protection decisions. |

Collector uncertainty is explicit. Windows records RSS, private bytes, threads and handles;
macOS records RSS, physical footprint where available, threads and file descriptors; Linux records
RSS/PSS where available, threads and file descriptors. The owner diagnostics supply compressed/PCM
high-water, automatic-upcoming, adaptation-scope/sample, open-request, operation-queue and page/query
counts. Missing owner or OS metrics are `BLOCKED` or a named limitation, never zero.

## Artifact rows and result policy

The immutable rows are Windows x64 MSI, Windows x64 NSIS, Linux x64 deb, Linux x64 AppImage,
macOS x64 DMG, and macOS ARM64 DMG. MSI, NSIS, deb and both DMGs require clean-install and upgrade
runs; AppImage requires clean launch. Each row records package SHA-256, source revision, application,
daemon, Rust, FFmpeg bindings, loaded FFmpeg libraries/ABI, OS/architecture, physical/VM status,
provider capabilities, physical output/device identities as hashes, and immutable raw-evidence URIs.

Allowed outcomes are `PASS`, `FAIL`, `BLOCKED`, and `NOT RUN`. Only `PASS` satisfies a gate.
Deterministic mocks may establish correctness but never replace installed physical observations.
Raw records must not contain tokens, authorization headers, credentials, URLs, usernames, home/profile
paths, raw portable server IDs, endpoint names, or provider response bodies.

