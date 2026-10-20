# Tune synchronization performance

HifiMule accepts an optional `sync_performance` object in the application's `config.json`. Edit this file while no synchronization is running, keeping existing fields such as the selected server and device ID. No application restart is required: manual and automatic syncs each take one validated snapshot when execution starts. Edits during a sync apply to the next run. Login, server switching and logout preserve these preferences; logout still removes credentials.

## File location

- Windows: `%APPDATA%\HifiMule\config.json`
- macOS: `~/Library/Application Support/HifiMule/config.json`
- Linux: `$XDG_DATA_HOME/HifiMule/config.json`, or `~/.local/share/HifiMule/config.json` when unset
- If `HIFIMULE_APP_DATA_DIR` is set, use `config.json` in that directory.

Add this object alongside existing properties (the example shows all defaults):

```json
{
  "sync_performance": {
    "copy_buffer_kib": 1024,
    "staging_max_tracks": 8,
    "staging_max_mib": 2048,
    "checkpoint_track_updates": 1000,
    "checkpoint_interval_seconds": 600
  }
}
```

Do not replace an existing config with only this example: merge the object into it. To restore defaults, remove the object or individual fields.

## Settings and tradeoffs

All values are integers; KiB is 1,024 bytes and MiB is 1,048,576 bytes.

| Field | Default | Inclusive range | Meaning |
| --- | ---: | ---: | --- |
| `copy_buffer_kib` | 1024 | 8–16384 | Size of each bounded read/write chunk when copying a staged file to mass storage, including USB sticks. MTP/WPD keeps its native transfer behavior. |
| `staging_max_tracks` | 8 | 1–32 | Maximum tracks admitted to disk staging, including the file being written to the device. |
| `staging_max_mib` | 2048 | 2048–32768 | Aggregate temporary-file disk budget. This does **not** allocate that amount of RAM. Each file still has a 2 GiB limit. |
| `checkpoint_track_updates` | 1000 | 1–5000 | Metadata mutations accumulated before a manifest checkpoint. |
| `checkpoint_interval_seconds` | 600 | 1–900 | Scheduled time between checkpoint attempts (up to 15 minutes); slow device writes may delay completion. |

A checkpoint occurs when either the update count or time threshold is reached. The timer also runs during a blocked transfer. Finalization flushes remaining metadata regardless of thresholds. Larger checkpoint values reduce metadata write frequency but leave more progress to recover after interruption. Verified writes, flushing, atomic publication and recovery protections remain enabled.

A larger copy buffer can reduce calls to the filesystem but consumes more working memory. A larger staging queue can help keep the device writer fed, at the cost of temporary disk usage; it does not add provider download workers or device writers. Values above defaults do not guarantee faster transfers. Transcoding, network bandwidth, the USB stick and its filesystem can each be the bottleneck. Audio quality and playback protection policies are unchanged.

Staging reservations account for expected file sizes, then actual received bytes. If inaccurate or unknown sizes fill the byte budget, staging waits cancellably while ready/writing files or active producers can free capacity. When every partial producer is blocked and no complete file can release bytes, the affected transfer returns a capacity error instead of waiting indefinitely. A file that cannot fit in the total budget also fails immediately. Increase `staging_max_mib` or reduce `staging_max_tracks` if that diagnostic occurs. Cancellation and completion clean up staging files and reservations.

## Validation and diagnostics

Missing fields use defaults silently. Invalid types, nulls, fractions, negative values or numbers outside the ranges default only the affected field; valid neighbors remain effective. A non-object tuning section defaults the whole section without breaking authentication. Unreadable or malformed JSON defaults performance settings and is not rewritten by synchronization; existing credential error handling still applies.

Daemon logs include a `[Sync] Performance` line showing the effective five settings for each execution. Invalid tuning produces a sanitized diagnostic naming the field and allowed range without logging config contents. Transfer summaries show staging and device-write rates, queue depth and waits; checkpoint logs show the reason and duration. Use these to identify where time is spent.

## Compare on your device

1. Record the default effective settings, application version, source server, device/filesystem, USB port, audio/transcoding profile and a fixed set of tracks with total bytes.
2. Run the same workload at least three times with defaults. Ensure each run actually copies the same tracks (an unchanged sync is not a transfer benchmark). Use a disposable test selection/device or restore the same starting device state between runs.
3. Change one setting, such as `copy_buffer_kib` to `64` or `256`, and repeat under the same conditions. Keep playback, competing disk activity and available temporary disk space consistent.
4. Compare median wall-clock time and staging/write rates, including checkpoint and finalization time. Alternate default and changed runs to reduce cache/warm-up bias. Record errors and cancellation behavior as well as throughput.
5. Keep a change only if the improvement is repeatable. Restore defaults if memory/disk use or recovery costs outweigh the benefit.

Temporary-directory correctness tests do not establish USB performance gains; measure with the actual memory stick.

## Follow a sync through the log

`daemon.log` includes `[SyncStep]` records without enabling debug logging. A start is written before each measured operation, including provider/device work during preparation. For example:

```text
[SyncStep] event=start run_id=<uuid> step_id=3 stage=basket_expansion
[SyncStep] event=end run_id=<uuid> step_id=3 stage=basket_expansion elapsed_ms=1250 outcome=success count=240
[SyncStep] event=link run_id=<uuid> plan_id=<uuid>
[SyncStep] event=link run_id=<uuid> operation_id=<uuid>
```

Match starts and ends using `run_id` and `step_id`. Stage names stay stable across manual, direct and automatic sync. Repeated stages (such as auto-fill slots or checkpoint writes) have distinct step IDs. Link records associate prepared plans and operations with the same run; IDs are generated internally. Credentials, configuration payloads and manifest fingerprints are excluded.

`elapsed_ms` uses a monotonic clock and measures wall-clock time, including waits. `count` is the available stage aggregate (resolved items for basket expansion, changed items for delta calculation, checkpoint updates, or errors for terminal execution); zero also means no aggregate is available. Outcomes are `success`, `ready` (preparation only), `admitted`, `failed`, `cancelled`, `skipped`, `noop`, or `interrupted`. A dropped future/scope reports `interrupted`, never invented success. Abrupt process termination cannot guarantee an end record.

The latest unmatched start identifies work still in progress. Preparation and execution have separate timers, so waiting for confirmation does not inflate execution time. Execution ends only after checkpoint and operation finalization, and file failures inside an otherwise successful executor result still report failure.

Compare the same stages across equivalent workloads. Durations overlap: preparation includes its child stages, transfer includes concurrent downloads and writes, managed deletions overlap transfer, and checkpoint timers can run during other work. **Do not sum overlapping stage durations.** Use `preparation` and `execution` for phase totals, then child timings and existing throughput summaries to locate bottlenecks. Optional reconciliation emits `skipped` when disabled; work not entered (for example forced-sync reconciliation or absent auto-fill slots) has no execution pair. Zero-count bulk stages may still report success when their empty work completed normally.

After the initial daemon-state check, a Subsonic sync with a saved sync token asks the provider for incremental changes before `preparation` starts; other cases skip this request. This request has its own `incremental_change_detection` run, with `target_validation`, `provider_resolution`, `provider_change_fetch`, and `change_enrichment` stages. Subsonic adds bulk `subsonic_indexes`, `subsonic_full_song_dump`, and `subsonic_album_fallback` stages for the applicable path. A stale token produces `fallback` on the fetch stage and a successful empty response, preserving the existing full-basket fallback. Tokens and provider error URLs are excluded from lifecycle records. This pre-preparation request and the later preparation request have separate run IDs; no RPC fields are added to correlate them.

Before change detection, the first state check after clicking Sync sends the optional diagnostic `syncStart: true` parameter on the existing `get_daemon_state` call. It emits `sync_start_check` before awaiting the snapshot, ending with `ready`, `waiting` (another pipeline is preparing), `attached` (an operation already exists), or `failed`. Subsequent polls and recovery checks omit this parameter, keeping routine polling quiet. This is a separate request/run from change detection and preparation, and does not change sync admission or RPC responses.

## Local encoding

Each server’s edit dialog includes **Sync encoding**: **Server (default)** or
**This computer**. Local encoding downloads original audio and converts it in the
daemon only when the selected device profile requires conversion. Choose it when
the local computer encodes faster; it downloads more bytes when the original is
larger than the converted file. Playback is unaffected. The setting is stored per
server, applies to manual and automatic sync, and is captured for each admitted
sync, including mixed-server baskets. Changing location alone does not replace
compatible device files.

Set the machine’s encoding concurrency alongside other local tuning in the
app-data `config.json` (merge this property into your existing object):

```json
{
  "sync_performance": {
    "local_encoding_workers": 0
  }
}
```

Omitted or `0` selects `max(1, available CPU threads - 1)`. Integers `1` through
`64` cap simultaneous encoding jobs. Invalid values fall back to automatic
independently of other fields; diagnostics never include configuration values.
Edits apply to the next sync and survive login, server switching and logout.
Active runs share one daemon-wide CPU budget; overlapping snapshots use the
strictest active limit. Each decoder and encoder starts with one codec thread.

Downloads, native encoding workers and the serialized device writer overlap.
Files and their original/output headroom share the staged-track/byte limits;
output capacity is reserved before downloading to avoid conversion deadlocks.
A file which cannot fit fails with a staging error. A bounded sample FIFO avoids
whole-track PCM buffering. Cancellation joins native workers before removing
temporary files. Completed output is reopened and checked before writer admission.

Supported targets include MP3, AAC in M4A/MP4 or ADTS, FLAC, ALAC in M4A/MP4,
Vorbis or Opus in Ogg, and PCM16/24/32 or float32 in WAV. Use device-wide
`MaxAudioSampleRate`, `MaxAudioChannels` and `MaxAudioBitDepth` ceilings.
Unsupported codec/container combinations and compatibility conditions fail
preflight with an actionable error; HifiMule never silently switches encoding
location or codec. Opus files decode at 48 kHz and cannot meet a lower sample-rate
ceiling. Supported containers retain source tags and attached cover artwork.

Logs report download time, local encoding wait/work time, encoded audio seconds
per wall second, staged bytes, queue depth and writer idle/write time. Compare
workers `1`, `2`, `4` and automatic on representative MP3/AAC/FLAC selections;
use total sync duration and validate the output. More workers may stop helping
when network throughput, staging headroom or device writes become the bottleneck.
