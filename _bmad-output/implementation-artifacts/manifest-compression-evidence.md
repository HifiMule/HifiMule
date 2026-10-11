# Manifest compression implementation evidence

Date: 2026-10-11. Implements [the approved spec](./spec-compress-device-manifests.md).

## Behavioral verification

The final daemon suite passed **1,441 unit tests and 5 contract tests**, with zero
failures and 10 ignored unit tests (including the explicit benchmark). It ran
serially because the default concurrent suite exposed an existing RPC test race:

```powershell
rtk node scripts/build-daemon.mjs test -p hifimule-daemon -- --test-threads=1
rtk node scripts/build-daemon.mjs check -p hifimule-daemon
```

Local output: `target/manifest-full-test-serial.log` and `target/manifest-check.log`.
The controlled build check passed; existing dead-code warnings remain.

Behavioral tests cover legacy migration, failed writes, postcommit cleanup failure
and durable dirty/pending recovery, compressed authority over legacy data, MTP
physical and portable cache identities, raw Windows WPD identifiers, corruption,
exact full-field reconstruction, duplicate order, Unicode, u64/i64 boundaries,
metadata fractional tokens, decoded and compressed size ceilings, checkpoint
thresholds, rollback and reconnect fences. New migration, cache precedence,
recovery and numeric/size-boundary regressions were observed failing before their
production fixes. Small limits exercise size boundaries without a 512 MiB allocation.

## Reproduce the Rust benchmark and inspector fixtures

```powershell
rtk node scripts/build-daemon.mjs test -p hifimule-daemon --bin hifimule-daemon --release manifest_codec_benchmark_and_rust_fixtures -- --ignored --nocapture
rtk node --test scripts/tests/inspect-manifest.test.mjs
rtk node scripts/inspect-manifest.mjs target/manifest-compression-rust.json.zst
```

The ignored Rust test generates 45,522 deterministic typed tracks and 45,522
pending IDs itself; no untracked input or external library catalog is required. It asserts
decoded state equality with legacy pretty JSON, writes the large compressed output
under ignored `target/`, and regenerates the small checked-in Rust fixtures at
`scripts/tests/fixtures/manifest-rust.json` and `.json.zst`. The fixtures include
all model fields, duplicate track/recovery IDs, Unicode, fractional settings,
u64::MAX and i64::MIN. The Node test compares all fields using exact numeric tokens
and verifies that inspection leaves both source formats unchanged.

Local release output: `target/manifest-benchmark-release.log`. Measurements use
the repository's actual release profile (`opt-level = "z"`, LTO, one codegen unit)
on Windows x64, Intel Core i9-9900K at 3.60 GHz, Rust 1.98.1 and Node.js 24.19.0.
One observation measures synchronous serialization/compression and parsing/decode;
it excludes snapshot cloning, worker scheduling and device/cache I/O. Synthetic
metadata repeats across tracks, so real-library sizes and timings will vary.

## Release measurement and inspection results

| Format | Bytes | MiB | Encode CPU | Decode CPU |
| --- | ---: | ---: | ---: | ---: |
| Ordinary pretty JSON | 38,723,346 | 36.929 | 87.068 ms | 161.123 ms |
| Column JSON + checksummed Zstd-9 | 1,937,120 | 1.847 | 358.077 ms | 115.777 ms |

The compressed file is **95.0% smaller** (about 20 times smaller). Encoding adds
about 271 ms of CPU in this observation; copy-time savings depend on the device's
actual transfer speed. No device transfer speed or memory-usage claim is inferred
from this CPU benchmark.

The refreshed release benchmark passed (one explicitly selected ignored test).
The Node inspector suite passed **10/10 tests** against the regenerated Rust fixtures;
output is in `target/manifest-inspector-tests.log`. Inspection of the release-produced
large frame emitted ordinary JSON with **45,522 tracks, 45,522 pending IDs and dirty
state preserved**, into the separate ignored `target/manifest-compression-inspected.json`.
Windows PowerShell 5 redirection uses UTF-16LE; this affects the exported text encoding,
not the inspector's UTF-8 stdout or source frame.

The final whitespace check passed using the repository's existing mixed line endings:

```powershell
rtk proxy git -c safe.directory=C:/Workspaces/HifiMule -c core.whitespace=cr-at-eol diff --check
```

## Patches following formal review

The parent task completed the formal acceptance audit and classified four findings
for local correction. All are implemented within the approved scope:

- Ordinary MSC cleanup failure retains previous/snapshot recovery evidence, without
  manufacturing dirty state for clean settings or basket updates.
- Manifest-only publication reads back exact compressed bytes and syncs the parent
  before legacy retirement. Successful deletion gets a further directory sync.
  Verification/durability failures are typed as uncertain publication, preserve
  legacy data and restore prior recovery evidence without retiring legacy. The
  platform helper uses Unix directory fsync and remains a non-Unix no-op; Windows
  has file sync/readback, without an additional directory durability guarantee.
- Inspection validates known metadata types, including recovery, basket and settings,
  preserves legacy defaults and raw numeric tokens, rejects simultaneous canonical
  and legacy aliases for each media/playlist path, accepts empty metadata track
  arrays consistently with Rust, and reads at most the input ceiling plus one byte
  through a single opened descriptor. Small bounds test exact/over/multiple-chunk
  reads without allocating 512 MiB.
- Ordinary updates move their snapshot into the blocking encoder, retaining only
  ID, dirty flag and pending IDs needed for logging and rollback.

New clean-state, corrupt publication, durability and inspector regressions were
observed failing before fixes. All 133 focused device tests passed (one ignored),
and the obsolete `MissingDeleteDeviceIo` double was updated to retain manifest bytes
for readback while preserving its already-absent media-delete behavior. Its focused
sync test passed afterward.

The next default full run had 1,440 passes and one failure in the previously recorded
`rpc::tests::rejected_control_does_not_supersede_pending_selection` global-ticket
race. Its failure output is preserved at `target/manifest-full-test-default-race.log`.
That case passed in isolation, and the complete serial suite passed with the counts
above. Playback/RPC production logic was not changed. A fresh controlled build check
passed after the review patches; existing warnings remain. Broader discovery/cache
issues remain in the parent-owned [deferred work](./deferred-work.md).

The codec is unchanged by these review patches, so the actual release benchmark
above remains applicable. Large-file inspection was repeated with the updated
inspector and again restored all 45,522 tracks/pending IDs and dirty state. The spec
remains `in-review` for the parent task's final reconciliation.
