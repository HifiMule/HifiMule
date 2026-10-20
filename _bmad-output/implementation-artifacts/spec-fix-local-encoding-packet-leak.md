---
title: 'Fix local encoding packet-buffer leak'
type: 'bugfix'
created: '2026-10-20'
status: 'done'
route: 'one-shot'
baseline_commit: '013a991'
---

# Fix local encoding packet-buffer leak

## Intent

**Problem:** Local sync conversion reused one FFmpeg demux packet across reads without releasing its previous payload. The controlled FFmpeg 9.0.2 runtime overwrites those references, so native memory accumulates across completed tracks; the user reported progressive growth to approximately 15 GB after increasing staging slots.

**Approach:** Scope each input packet to one read-loop iteration so Rust releases it on successful processing, skipped streams, tolerated trailing metadata and errors. Keep profiles, staging budgets, cancellation and output validation unchanged. A bounded native-reference probe fails against the old loop (refcount 2 instead of 1) and passes with the fix, including successful conversion, output-capacity failure and MP3 trailing metadata. An isolated native benchmark measures repeated conversions at eight and sixteen concurrent jobs; its evidence is separate from full-device sync claims. The full daemon suite passed 1,621 unit tests and five contract tests, with 14 ignored. Independent review's output-fixture and tail-probe coverage findings were patched; no findings remain or were deferred.

## Suggested Review Order

- Release each caller-owned input packet before another demux read can overwrite its buffer.
  [local.rs:913](../../hifimule-daemon/src/transcoding/local.rs#L913)

- Verify normal and failed conversions release native input references.
  [local.rs:1119](../../hifimule-daemon/src/transcoding/local.rs#L1119)

- Inspect bounded, thread-local native-buffer probes and their cleanup.
  [local.rs:1649](../../hifimule-daemon/src/transcoding/local.rs#L1649)

- Reproduce isolated repeated-conversion memory measurements independently of the device pipeline.
  [local_memory_benchmark.rs:21](../../hifimule-daemon/src/transcoding/local_memory_benchmark.rs#L21)

- Review measured workload, peak-memory samples and limitations.
  [local-encoding-memory-evidence.json](local-encoding-memory-evidence.json)
