# Local sync transcoding: runtime and concurrency design

Approved implementation details accompanying `spec-per-server-local-sync-transcoding.md`. Encoding location remains per server; CPU scheduling belongs to the local daemon.

## Required FFmpeg capabilities

| Target | Encoder | Output muxer | Current controlled package |
|---|---|---|---|
| MP3 | `libmp3lame` | `mp3` | Both missing |
| AAC in M4A/MP4; raw AAC | `aac` | `ipod`/`mp4`; `adts` | Encoder, `ipod`/`mov` and `adts` present; add explicit `mp4` |
| FLAC | `flac` | `flac` | Present |
| ALAC in M4A | `alac` | `ipod`/`mp4` | Encoder missing |
| Vorbis in Ogg | `libvorbis` | `ogg` | Both missing |
| Opus in Ogg/Opus | `libopus` | `opus`/`ogg` | Both missing |
| PCM in WAV | `pcm_s16le`, `pcm_s24le`, `pcm_s32le`, `pcm_f32le` | `wav` | Only float encoder present; WAV muxer missing |

Keep existing playback encoders/muxers and input decoders/demuxers. Existing inputs include MP3, AAC, FLAC, ALAC, Vorbis, Opus, PCM/WAV/AIFF and WMA/ASF. A custom source requiring another decoder must fail clearly instead of producing a partial conversion.

Add FFmpeg build switches `--enable-libmp3lame`, `--enable-libopus`, `--enable-libvorbis` and enable the encoders/muxers above. Provision pinned LAME, libopus, libogg and libvorbis/libvorbisenc dependencies across macOS, Linux and Windows, including native closure and notices. Configure receipts must invalidate incompatible caches. Preserve the current distribution license policy.

Retain `libavcodec`, `libavformat`, `libavutil` and `libswresample`, file I/O and seeking. Conversion needs resampling, channel remapping/downmixing and sample-format conversion, plus buffering arbitrary decoded frame sizes into encoder frames. Use the libraries already linked by the daemon; the implementation does not require an installed FFmpeg command-line program.

Preserve tags and attached artwork by copying supported image packets without image re-encoding. Inspect actual output by reopening it; test codec/container, rate, layout, effective precision, duration, tags, artwork and bitrate behavior. A codec's supported rates, layouts and precision may be narrower than a profile's maxima. Choose a supported value within the requested ceilings; reject impossible requirements. For lossy output, precision means decoded/sample representation rather than a lossless bit-depth guarantee.

Profiles are arbitrary JSON, so universal support for every possible codec or incompatible codec/container combination is impossible. Support generic resolution of the listed audio targets and aliases, validate relevant profile constraints and reject unsupported targets before downloading where possible. HTTP streaming flags do not become file-encoding options. Do not silently ignore a constraint that changes required output compatibility.

## Pipeline

`download admission → original-file queue → shared encoder pool → completed-file queue → existing device writer`

The current implementation has one sequential producer per source server. Awaiting encoding inside that producer would serialize local conversions for a single-server basket. Split scheduling so a producer enqueues each completed original and continues downloading while other tracks encode. Server-encoded or direct-copy files bypass the encoder pool.

Use one daemon-wide semaphore/CPU budget across servers and concurrent syncs. Each admitted encoding task owns independent FFmpeg contexts and runs on a blocking worker; do not execute codec work on Tokio's async executor. Acquire an encoding permit before spawning, so the blocking executor does not accumulate waiting tasks. Await active workers during cancellation before deleting their files or releasing reservations.

Start automatic parallelism at `max(1, available_parallelism - 1)`. Configure the override in the existing app-data `config.json`, alongside other local sync performance settings:

```json
{
  "sync_performance": {
    "local_encoding_workers": 0
  }
}
```

Merge this field into the existing object, preserving other settings. Omission or integer `0` selects automatic sizing; integers 1–64 set an explicit maximum number of concurrent encoding jobs. Invalid types, nulls, fractions, negatives and out-of-range values fall back to automatic with a sanitized diagnostic, independently of neighboring fields. Read once at sync execution admission, just like existing tuning; edits apply to the next run without a restart. Preserve the field during login, server switching and logout. Do not add a UI control or per-server database column for this machine-level worker budget. Coordinate snapshots through the shared pool rather than creating separate per-sync CPU budgets.

This is a starting policy, not a claim of optimal hardware utilization. Actual jobs are further bounded by staged-track slots, disk headroom and runnable work. Start codec contexts at one thread per track; additional internal decoder/encoder threads must consume the same CPU budget. Independent tracks provide parallelism even when a codec cannot parallelize a single encode.

Retain existing per-server download fairness and manual-before-auto-fill admission. Decoupling allows one server to supply multiple encoder jobs without increasing simultaneous HTTP requests initially. Add download concurrency only if measurements show source fetching is the bottleneck; CPU-bound conversion does not benefit from arbitrary additional downloads.

Both queues carry file paths and metadata, never whole audio files or unbounded PCM. Share existing staged-track and byte limits across downloads, originals, active conversions and completed outputs. Current defaults are eight tracks and 2 GiB per sync. Reserve conversion output headroom before admitting more downloads, prioritize draining existing originals, and detect files that cannot fit. A full original queue must not consume all disk reservations and prevent any encoder from finishing. Release original bytes after successful conversion and retain output reservations until writer completion.

Allow independent conversions to finish out of order; preserve writer-authoritative destination ownership/reuse, manifest safety and playlist order derived from the plan. Resolve destination collisions through the writer, not worker completion order.

## Performance verification

Verify multiple conversions from one server overlap, downloading proceeds during conversion, and writing proceeds while later tracks encode. Enforce the global CPU limit across servers/syncs. Record download, encoding and write times separately; include encoded-audio-seconds per wall-second, queue depths, wait times, staged bytes and writer idle time. Benchmark worker counts 1, 2, 4 and automatic on representative MP3/AAC/FLAC targets; select policy using end-to-end sync time without sacrificing output requirements.

Sources: [FFmpeg encoders](https://ffmpeg.org/ffmpeg-codecs.html), [muxers](https://ffmpeg.org/ffmpeg-formats.html), [resampler](https://ffmpeg.org/ffmpeg-resampler.html), and [LAME wrapper capabilities](https://ffmpeg.org/doxygen/trunk/libmp3lame_8c_source.html). Runtime presence statements come from `hifimule-daemon/audio-runtime.json`.

## Implementation evidence — 2026-10-20

The controlled runtime was rebuilt and capability-verified on macOS ARM64. Native
round-trip tests exercised MP3, AAC/M4A, ADTS, FLAC, ALAC, PCM16/24/32/float WAV,
Vorbis/Ogg and Opus. Additional fixtures verified tags and PNG cover-packet
preservation in MP3/FLAC/M4A/Ogg/Opus (including Ogg/Opus back to FLAC), 96 kHz/24-bit stereo to 44.1 kHz/16-bit mono,
invalid media, output-capacity exhaustion, and cancellation cleanup. Artwork is
bounded to 16 MiB total; known-size staging reserves 24 MiB metadata headroom
to include base64-expanded Ogg picture comments and tag/muxer overhead. Unknown
lossless source properties use an exclusive whole-budget reservation; original
bytes and encoded output remain bounded together within that held reservation.

The initial implementation regression run passed **1,613 unit tests and 5 contract
tests**, with 13 ignored tests (12 existing plus the explicit throughput
benchmark). Automatic sync, mixed-server routing, overlapping original downloads,
multiple encoders from one source, serialized writing while later workers remain
active, shared pool limits and active cancellation were covered. The UI production
build, 9 selector/server-hub behavior tests, 7 localization tests and 90 runtime
packaging tests passed; one Linux-host-only packaging check was skipped on macOS.
A bounded visual pass and confirmation used the real ServerHub fixture: settings
copy, collapsed parent menu, selector/default/help and option selection passed.

One repeated full run made concurrently with the CPU benchmark timed out in
`device::tests::test_select_device_and_update_manifest_do_not_deadlock`
(`Elapsed(())`; 1,612 passed, 1 failed). A subsequent full run exposed test-only
permit interference between a gated two-worker fixture and the one-permit pool
fixture; their test gates now share a lock. The isolated final full run passed.
No production timeout threshold was changed.

### Isolated synthetic pipeline benchmark

Command: `rtk npm run build:daemon -- test -p hifimule-daemon
local_encoding_throughput_benchmark -- --ignored --nocapture`.

This is **loopback HTTP plus a temporary MSC filesystem**, eight generated
30-second stereo WAV tracks per run (240 audio seconds), converted to 44.1 kHz
stereo with a 16-bit ceiling and 192 kbps for lossy targets. The elapsed time
includes download, staging, native conversion, verified device writes and
manifest finalization. Fixture generation is outside each elapsed interval.
The isolated benchmark passed all 12 cases; no other test workload ran alongside
these measurements. `0` means automatic; at most eight jobs can be staged.

| Target | 1 worker | 2 workers | 4 workers | Automatic |
|---|---:|---:|---:|---:|
| MP3 | 1.250 s | 0.657 s | 0.467 s | 0.457 s |
| AAC/M4A | 1.861 s | 1.052 s | 0.708 s | 0.555 s |
| FLAC | 0.461 s | 0.353 s | 0.347 s | 0.383 s |

The results support independent-track parallelism and retaining a configurable
automatic default. They are not a physical USB/MTP, remote-server bandwidth,
thermal, or battery recommendation. Four workers slightly outperformed automatic
for this synthetic FLAC input. Real libraries, hardware and network conditions
should determine a user's override.

Linux, Windows/MSVC and macOS x86-64 native dependency builds/installers could not
be executed on this host. Their pinned source/static-library/pkg-config/receipt
contracts passed the available packaging tests; those platform-native builds
remain release validation requirements. Unsupported profile compatibility
conditions are rejected explicitly rather than silently ignored or sent back to
the server. No commits, pushes or remote repository changes were made.


### Independent-review corrections

Six deduplicated findings were fixed within the approved intent. Local profile
normalization now retains actual device ceilings separately from observed source
properties: 44.1 kHz input can become Opus at 48 kHz, explicit PCM24/32/float can
expand a 16-bit source representation when permitted, and FLAC/ALAC retain known
16-bit source precision under a 24-bit device maximum. Original URL, HTTP and
non-audio response failures preserve manual cancellation versus Auto-Fill warning
and handled-item behavior. Ogg/Opus artwork uses standard FLAC picture blocks in
METADATA_BLOCK_PICTURE comments; source stream tags also survive reverse conversion.
Unknown lossless input properties no longer introduce fabricated maximum source
rates/channels or treat target size estimates as original download size. They
reserve the bounded whole staging budget and derive available output bytes from
the completed original, with no reservation-growth wait between original/output.

Focused regressions passed for source precision/rates, all six original-failure
policy cases, artwork/tag conversions in both directions, and two consecutive
unknown-property five-minute lossless plan entries whose target estimates exceed
the default budget. The actual originals are small generated fixtures; this
checks admission and bounded draining rather than five minutes of encoding.

The single final daemon regression after these corrections passed **1,616 unit
tests and 5 contract tests**, with 13 ignored tests and no failures. Unit tests
completed in 29.38 seconds. Packaging and UI results above remain applicable;
review corrections affected the native encoder and sync producer only. Final
`git diff --check` passed. Platform-native build limitations remain unchanged.
