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
    "copy_buffer_kib": 8,
    "staging_max_tracks": 2,
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
| `copy_buffer_kib` | 8 | 8–16384 | Size of each bounded read/write chunk when copying a staged file to mass storage, including USB sticks. MTP/WPD keeps its native transfer behavior. |
| `staging_max_tracks` | 2 | 1–32 | Maximum tracks admitted to disk staging, including the file being written to the device. |
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
