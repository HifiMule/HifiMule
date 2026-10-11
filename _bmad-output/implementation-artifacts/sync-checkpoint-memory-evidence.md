# Large-sync follow-up evidence

Baseline production commit: `ef69a49fdefa68bc4576860455167d76b90c2058`.

## Real workload

The user reported approximately 449 MB daemon memory during a mixed explicit-selection/Auto-Fill sync. The installed executable contains the prepared-plan RPC implementation; this was not an old-binary comparison. Read-only process samples showed roughly 515–519 MiB private bytes. A device manifest contained 45,522 synced tracks, 45,539 pending identities, 56 playlists, and 35,570,415 encoded bytes.

The previous executor-only test omitted UI state polling. `DestinationHub` requests daemon state every second throughout sync. Each full response clones the selected manifest, clones every connected manifest for small metadata, creates a full per-song JSON tree, and then encodes the JSON-RPC response. `BasketSidebar` itself pauses its two-second poll during sync, but this independent poller continues.

## User-confirmed real-sync improvement

On 2026-10-11, the user reported a massive reduction during their real sync, including the UI, and supplied a Windows Task Manager screenshot. Compared with their earlier screenshot after the first memory fix:

| Process | Earlier sync | Updated sync | Reduction |
| --- | ---: | ---: | ---: |
| hifimule-daemon | 449.3 MB | 78.3 MB | 82.6% |
| HifiMule app | 280.9 MB | 8.2 MB | 97.1% |
| WebView2 Manager | 640.7 MB | 205.1 MB | 68.0% |

These are the memory figures displayed by Task Manager at two points in the user's real workload, rather than matched peak measurements or an hours-long trace. The updated screenshot shows the daemon actively writing at 7.9 MB/s. Its 78.3 MB reading is close to the controlled executor's approximately 77 MiB working set, although the measurements use different reporting conventions.

The UI improvement is consistent with compact polling: full song manifests no longer cross the shared state RPC every second, reducing allocation and JSON processing in both the daemon and WebView. The screenshots confirm improvement across all three displayed process entries; they do not isolate each change's contribution.

Evidence supplied in this conversation: earlier `codex-clipboard-aec9a8d3-55b2-48a2-a92a-cca52358b76a.png`; updated `codex-clipboard-f400b077-a328-4cad-86c3-2fd0bb9e2ed4.png`.

## Reproduced state-polling allocation

The ignored `rpc::tests::benchmark_daemon_state_retention` creates representative metadata, pending identities and 56 playlists. It calls the real state handler and encodes the actual response before the hold and once per second during it, dropping each response/body between polls. This isolates the previously omitted polling workload; it does not include audio copying or provider discovery.

At 45,522 tracks, the full-state baseline held for 20 seconds:

| Metric | Full polling baseline |
| --- | ---: |
| Peak hold private memory | 291.3 MiB |
| Peak hold working set | 297.2 MiB |
| Last hold private memory | 202.8 MiB |

The fixture fell to 7.2 MiB private memory after polling stopped and all fixture state was dropped. This confirms large transient live allocations from polling; it does not establish a permanent allocation leak. Concurrent durable manifest snapshots and real provider data can add to these figures.

Trace: `target/sync-state-memory/current-45522-20261011-115835-515.json`. Diagnostic baseline executable SHA-256: `8D38C7455A911D7AC30E3F6C27DF0F6FC16CFB4BC8BBF1A8ADB77F3D19EFD601`; snapshot: `target/sync-state-baseline/hifimule_daemon.exe`.

## Finalization report

Audio staging cleanup succeeded at 11:10:10. The log advanced to an interrupted sync at 11:53:52. The user reported `Per-ID-change manifest write failed: Access...`. Thus final ID metadata persistence failed after audio transfer. The previous implementation rewrites the entire manifest for each ID change as well as each copied file. The exact reason for Windows access denial is unconfirmed.

During diagnosis, the live manifest was read for aggregate statistics. A Windows reader without delete sharing can temporarily block atomic replacement, so this read may have contributed to the access error. No further real-device reads or mutations are used for testing; lock behavior is reproduced on temporary fixtures.

The user explicitly requested periodic checkpoints instead of per-file manifest writes, suggested ten minutes and then 1000 tracks, and requested checkpoint logging. The follow-up implements the earlier of both bounds plus finalization flushes, retaining a durable initial dirty/pending marker.

## Regression evidence before implementation

- A temporary Windows reader opened with read/write sharing but without delete sharing reproduced error 5 (`Access denied`) during canonical manifest replacement. A second fixture confirmed the old code preserved the canonical bytes but leaked its temporary file after retry exhaustion. These fixtures use temporary directories, not the user's device (`target/checkpoint-lock-red.log`).
- The count-boundary regression observed 99 immediate manifest mutations on disk, where the requested policy requires no intermediate full write before the 100th update (`target/checkpoint-count-red.log`).
- A rediscovery fixture reused the same backend `Arc` with a newer observation. The initial IO-only final-commit check incorrectly accepted it and cleared replacement recovery state; the follow-up fence therefore carries the original path/observation through admission, staging and clean finalization (`target/checkpoint-reused-arc-red.log`).
- The measurement script supports explicit executor state polling and compact representation, records those modes, and passed the PowerShell parser check. Final measurements must distinguish the isolated polling fixture from the combined executor workload.

## Matched full versus compact polling

Fresh processes used the same 45,522-track fixture, ten preparation polls, and a 20-second transfer hold. Both called the actual state handler and encoded the JSON-RPC response. Only the current process requested compact state.

| Transfer-hold metric | Baseline full state | Compact state |
| --- | ---: | ---: |
| Peak private memory | 291.27 MiB | 56.74 MiB |
| Peak working set | 296.21 MiB | 70.45 MiB |
| Last private memory | 169.84 MiB | 56.65 MiB |
| Encoded response | 31,699,264 bytes | 2,352 bytes |

Peak private memory fell **80.52%** in this isolated polling workload. These are debug test processes, not a prediction of the installed daemon's total RAM. Real provider discovery, audio adaptation, and concurrent durable writes can add allocations.

Traces: `target/sync-checkpoint-state-memory/baseline-45522-20261011-121751-741.json` and `current-45522-20261011-123742-393.json`. Baseline SHA-256: `8D38C7455A911D7AC30E3F6C27DF0F6FC16CFB4BC8BBF1A8ADB77F3D19EFD601`. Compact executable SHA-256: `2F71FBAD4B3898AC3449ECACF874B4F7EE7E2F5765F0D2E78A9837ED02EBD791`, preserved under `target/sync-checkpoint-measure/`. This compact snapshot precedes the final additional connection-token fence; the isolated polling fixture does not exercise that fence.

## UI verification

TypeScript checking and Vite production build passed (416 modules). UI tests: 59 passed, two failures; script tests: 309 passed, ten skipped. The two UI failures are the previously documented Windows `/C:/...` URL path issues in `radioDefaults.test.mjs` and `serverSwitchBrowse.test.mjs`, unchanged by this work. Logs: `target/checkpoint-ui-broad.log`, `target/checkpoint-script-tests.log`.

## Executor with checkpoint and periodic polling

The fixture uses the actual executor with 45,522 existing tracks, 45,522 pending additions, temporary MSC storage and a local mock provider. It blocks the 101st audio write after 100 completed transfers, then calls the real state handler and encodes its compact response once per second for 120 seconds, sharing the executor's authoritative `DeviceManager`.

The fixture verified exactly **two** manifest writes before the hold: the initial dirty/pending marker and one 100-update checkpoint. The checkpoint encoded 35,757,720 bytes. The hold recorded 1,048 samples spanning 119.68 seconds:

| Metric | Final executor plus compact polling |
| --- | ---: |
| Private-memory range during hold | 74.22–74.50 MiB |
| Peak hold working set | 77.39 MiB |
| Preparation peak private memory | 145.70 MiB |
| Final flush peak private memory | 128.20 MiB |
| Last cleanup private memory | 7.03 MiB |

Cancellation released the blocked write and logged a finalization checkpoint with one additional completed update. The test passed. This verifies sustained retention in this controlled executor scenario; its peaks are brief preparation/finalization allocations. It excludes actual server discovery and large audio payloads and does not establish the real installed daemon's absolute RAM ceiling. There is no matched old full-polling executor run, so the 80.52% comparison above applies only to the separate polling fixture.

Trace/log: `target/sync-checkpoint-executor-memory/current-45522-20261011-124509-808.{json,log}`. Executable SHA-256: `70CF04D7A0367FA592E304045937DC86144B59DD36C9436B67EE14EBDB339B4E`, preserved under `target/sync-checkpoint-final-measure/`. This snapshot precedes three review fixes for indexed provenance, exact copied-ID lookup and pending initialization/removal. Those paths are not exercised by this transfer hold.

After all review fixes, a fresh linked executable repeated the same 45,522-track fixture with a 20-second hold. It again verified two manifest writes before holding. Private memory stayed at **73.95–74.22 MiB** across 176 samples spanning 19.94 seconds; peak working set was 77.48 MiB. Preparation peaked at 145.73 MiB private bytes, finalization at 131.05 MiB, and cleanup ended at 7.26 MiB. The fixture passed. Trace/log: `target/sync-checkpoint-reviewed-memory/current-45522-20261011-130823-734.{json,log}`. Final executable SHA-256: `E897401FCC0EB1CC4CBCB944425B882040ECD6B75907F72A3E0313079AFD2B95`, preserved under `target/sync-checkpoint-reviewed-measure/`.

Example captured checkpoint logs:

```text
[Sync] Manifest checkpoint start reason=count updates=100 elapsed_ms=11272 manifest_bytes=35757720
[Sync] Manifest checkpoint result reason=count updates=100 elapsed_ms=11313 manifest_bytes=35757720 duration_ms=1441 result=ok error=
```

`duration_ms` covers checkpoint validation, encoding and persistence; the start line marks the beginning of persistence after encoding determines the size. Initial and final clean commits use `reason=initial` and `reason=clean`. Post-transfer phase logs identify directory cleanup, ID updates and playlist generation.

## Daemon verification

Final full suite after review fixes: **1,424 unit tests and five contract tests passed**, zero failures, nine ignored tests. `cargo check` passed with 30 existing warnings. Tests cover 99/100-update boundaries, timer reset after a count checkpoint, a timer while persistence is blocked, finalization/drain, transient and exhausted Windows locks, staged recovery after failure, stale removal, copied identities and a reused backend with a newer observation. New review regressions cover exact copied-ID commit selection, pending publication/removal coordination, newer pending arrival preservation, and a 1,301-row provenance transition with duplicate IDs and unchanged origins producing exactly 13 writes. Logs: `target/checkpoint-review-final-tests.log`, `target/checkpoint-review-final-check.log`. `git diff --check` passed. Independent reviewers confirmed the three patches; see `sync-checkpoint-review.md`.
