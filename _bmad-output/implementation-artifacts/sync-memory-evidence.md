# Large-library sync memory evidence

Baseline: `0ce0d8c348e13cc5b95c1c76862779de625ded00`.

Measured on Windows x64 with the controlled FFmpeg 9.0.2 runtime and debug Rust test executables. Each case starts a fresh process. The real provider sync executor runs against a local mock HTTP provider and a temporary device; its first audio write is blocked. The fixture creates N existing manifest entries and N pending additions, with representative names, albums, artists and etags. The baseline retains its typed delta while executing; the revised executor consumes it into the temporary plan and compacts its target before execution.

These are controlled retention measurements, not a prediction of the user's release-build memory. The fixture excludes real provider library discovery and large audio payloads. It pauses after the first write begins, holds for the stated interval, then cancels and drains the executor. Authoritative manifest data remains proportional to the existing library.

## Transfer measurements

| Pending additions / existing tracks | Baseline private memory | Revised private memory | Reduction |
| --- | ---: | ---: | ---: |
| 5,000 / 5,000 | 29.9 MiB | 12.1 MiB | 60% |
| 40,000 / 40,000 | 203.2 MiB | 50.3 MiB | 75% |
| 80,000 / 80,000 | 401.7 MiB | 92.7 MiB | 77% |

Peak private bytes during a 20-second blocked transfer. Working sets for revised cases were 29.4, 65.2 and 106.7 MiB respectively. Preparation private peaks for revised cases were 19.4, 103.8 and 199.8 MiB. Preparation is measured separately and is allowed to peak.

The additional 120-second 40,000-track holds remained stable: baseline private bytes peaked at 203.2 MiB and ended at 203.0 MiB; revised peaked at 50.1 MiB and ended at 50.0 MiB. This is evidence of retention stability while waiting, rather than evidence from an hours-long real-device run or manifest growth during a complete sync.

## After durable file commits

A further matched 40,000-track case lets five audio files and their per-file manifest updates complete, then blocks the sixth audio write. This includes allocator retention from the durability snapshots. The baseline held for 20 seconds; the revised implementation held for 120 seconds.

| Metric | Baseline | Revised |
| --- | ---: | ---: |
| Peak transfer private memory | 231.6 MiB | 71.6 MiB |
| Last transfer private memory | 231.3 MiB | 71.4 MiB |
| Peak transfer working set | 220.4 MiB | 73.7 MiB |

Private memory is about **69% lower after commits** and remains stable across the revised two-minute hold. The preparation phase in this variant includes the five warm-up commits; use the initial cases above for preparation-only peaks.

Raw traces: `target/sync-memory-measurements/baseline-40000-20261010-233424-356.json` and `current-40000-20261011-000142-686.json` in the same directory. The final revised executable SHA-256 is 36DD3477B0FACFD82E7EF6BB176A000BED48529AB166CBBA9856F3B2A17EF4BC; both hashes are recorded in the trace files. The baseline test-only blocking helper was updated to match the revised fixture; baseline production code remains the recorded commit.

## Reproduction and raw evidence

The tracked current fixture is `hifimule-daemon/src/sync_memory_benchmark.rs`, included by the sync module tests. `scripts/measure-sync-memory.ps1` launches the named ignored benchmark, samples Windows private bytes and working set every 100 ms, records the executable SHA-256, and saves phase summaries and every sample. It refuses results if the test fails or does not reach the required phases. Run it through PowerShell with process-local `-ExecutionPolicy Bypass` if script execution is otherwise disabled.

Build the test executable through `scripts/build-daemon.mjs test -p hifimule-daemon --no-run`, using the repository's controlled runtime. For a matched baseline, extract the baseline commit into an ignored snapshot and add the same fixture metadata and blocking behavior, replacing plan construction with the original `execute_test_provider_sync` entry point. The snapshot, matching baseline fixture and executable are retained under `target/sync-memory-baseline`, `target/sync-memory-benchmark-baseline.rs` and `target/sync-memory-baseline-build` in this workspace.

Raw baseline traces are `target/sync-memory-baseline-{5000,40000,80000}.json`. Raw PowerShell traces and logs are under `target/sync-memory-measurements/`; current executable snapshot is `target/sync-memory-current/hifimule_daemon.exe`. These generated binaries and traces are ignored by Git.

## Verification

The final post-review regression run passed **1,410 daemon unit tests and 5 contract tests**, with 8 ignored (including the externally run memory benchmark). Daemon check passed with existing warnings. UI TypeScript and Vite production build passed (416 modules); targeted UI tests passed 18/18. Broad JavaScript tests passed 366 with 10 skipped and two failures in the previously documented Windows esbuild path harnesses (`radioDefaults.test.mjs`, `serverSwitchBrowse.test.mjs`). Those failures occur before test execution and the files are unchanged.

Final logs: `target/sync-review-lifecycle-final-tests.log`, `target/sync-review-lifecycle-final-check.log`, `target/sync-review-final-ui-build.log` and `target/sync-review-final-js.log`. A concurrent check encountered a Windows DLL staging lock while tests ran; the sequential retry passed. No production checks remain pending. Independent review fixes and the focused follow-up are documented in `sync-memory-review.md`.
