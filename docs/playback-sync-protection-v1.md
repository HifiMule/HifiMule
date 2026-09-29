# Playback/Sync Protection Policy v1

This document freezes the scheduler contract implemented by Story 16.13. Times are monotonic milliseconds. A policy change requires a new version and updated deterministic and coexistence evidence.

## Evidence and states

- Eligible evidence exactly matches playback adaptation v1: complete origin requests only; startup, cache, seek/range, partial, failed, and requests shorter than 250 ms are excluded. Histories retain at most 16 samples per source/representation and 64 scopes.
- Risk evidence uses samples no older than 60,000 ms and requires at least three eligible samples. It is actionable when at least two samples have compressed or PCM low-water below 100 ms, or the minimum delivery rate cannot sustain the representation at 125% headroom.
- Recovery begins only after a current 60,000 ms window contains at least three eligible, non-depleted samples whose minimum rate meets 175% headroom. The owner retains that qualified episode's first-sample anchor across later qualifying windows and releases protection only after the newest qualifying sample is at least 120,000 ms beyond the anchor. An insufficient, depleted, or under-headroom window resets the recovery anchor without releasing the still-current restriction.
- An owner-admitted snapshot carries session, generation, occurrence kind, portable server, representation, observation/expiry time, sample count, classification, and a sanitized reason code. It never carries a URL, credentials, headers, track ID, or provider request.
- `unknown`, `healthy`, `stale`, no playback, and playback whose source is absent from the sync set use the exact existing scheduler. Stop, end, session/generation/output replacement, seek generation, closed ownership, or evidence older than 60,000 ms invalidates a restriction. Pause retains it for no more than 5,000 ms.

## Scheduler decisions

| Decision | Delay | Reason code | Scope |
|---|---:|---|---|
| Normal | 0 ms | `healthy`, `unknown`, `stale`, `noActivePlayback`, `sourceNotInSyncSet`, or `deviceLimitedNoAction` | No producer is changed |
| Attributable protection | 500 ms | `attributableSharedSource` | Only the producer for the active portable server |
| Unattributable depletion | 100 ms | `unattributableOutputRisk` | One weakest delay for each future producer attempt |
| Recovery pending | Same as active protection | `recoveryPending` | Existing protected scope |
| Ineffective/minimum progress | Same bounded delay | `ineffectiveProtection` | Never escalates, pauses, or cancels sync |

Protection is checked before resolving a future item and again before every provider HTTP or URL retry. A delay is applied at most once for each item/retry admission, is cancellable, and never holds a staging count/byte permit. The snapshot is sampled again after the delay for current diagnostics and stale/recovery fencing; minimum progress makes the producer immediately eligible rather than applying a second delay to the same admission. Current provider requests, staged files, verified device writes, metadata/manifest updates, cleanup, and M3U work are never interrupted by this policy. Slow device writes and writer queueing are not playback evidence.

Risk becomes ineffective after 120,000 continuous milliseconds while protection is applied. This is a diagnostic state and minimum-progress guarantee, not an escalation. Valid recovery removes the delay for the next unresolved item.

## Diagnostics and evidence gate

Diagnostics report policy version, classification, sanitized playback evidence reason, scheduler decision reason, affected portable server (using the existing opaque server identifier), eligible sample count, observation age, decision delay, and ineffective duration. A weak-lifetime observer treats the final playback publisher disappearing as immediately closed and returns no snapshot. Diagnostics keep provider staging and physical writer rates separate and make no causal claim beyond the selected evidence rule.

The fixed healthy coexistence gate is a combined-run median sync throughput within 5% of the sync-only baseline, with zero protection decisions. Raw runs and variance must be retained. Thresholds are not tuned after results are observed. Deterministic results are not installed-platform certification; sustained Windows, macOS, and Linux evidence belongs to Story 16.14.
