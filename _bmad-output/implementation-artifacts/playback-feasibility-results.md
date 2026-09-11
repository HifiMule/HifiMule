# Playback feasibility results

Date: 2026-09-11. Targets: Windows, macOS, Linux.

This document distinguishes code inspection, synthetic tests, and real integration tests. The experiment is isolated in `experiments/playback-probe`; no production playback feature has been implemented.

## Repository findings

### Selection reuse

The pure `run_pipeline` function in `hifimule-daemon/src/auto_fill/pipeline.rs` is a suitable shared boundary. Playback needs independent configuration, track-count lookahead, session exclusions, and source-qualified identities. Current pure-engine candidate pools and history do not carry explicit server IDs. Add a safe aggregation boundary before mixing identical server-local IDs.

The current engine materializes candidate pools and supports device byte/duration budgets. Reusing it does not mean repeatedly fetching the entire library for each queued track. A bounded queue and a bounded/cache-aware catalog strategy are separate requirements. Existing local sync rotation/pity history must not silently become the persistent playback taste model the user rejected.

### UI and daemon lifetime

`hifimule-ui/src-tauri/src/lib.rs:554–563` kills an owned sidecar on UI exit. An already-running external daemon survives because the UI never owns it (`:418–426`). Linux normally follows the owned-sidecar path; this is a real gap for the desired close-window/keep-playing behavior. A durable user-session process with explicit Quit semantics needs design, including child logging/pipe ownership.

macOS uses a LaunchAgent and the daemon's main-thread Tao event loop. Its accessory activation policy and tray icon provide a menu-bar status item, not a daemon Dock menu. Windows production login startup is appropriate for playback; legacy Windows Service fallback remains in the code and is not equivalent to an interactive user process.

No existing media-key integration was found. A candidate adapter is [Souvlaki](https://docs.rs/souvlaki/latest/souvlaki/): macOS needs an application event loop, Windows an HWND, Linux a session D-Bus/MPRIS identity. These requirements have not yet been implemented or tested in HifiMule.

## Provider assessment

| Capability | Verified foundation | Missing or conditional |
|---|---|---|
| Bitrate selection | Subsonic stream URL includes format/maxBitRate; Jellyfin PlaybackInfo builds a device profile | Playback-specific offset/capability contract; real behavior under constrained bandwidth |
| Mid-track adaptation | Jellyfin audio API accepts startTimeTicks; OpenSubsonic has transcodeOffset extension | Sample alignment and buffer overlap; no universal seamless switch guarantee |
| Completed listens | Both provider adapters submit completed play history | Now-playing/session reporting, seek/skip semantics, actual server observation |
| Like/dislike | Jellyfin upstream supports nullable Likes separate from favorites | Current model discards Likes; Subsonic has stars/ratings rather than standardized binary dislike |
| Loudness | OpenSubsonic ReplayGain; current Jellyfin upstream gain fields | Fields not retained in normalized Song; installed-version availability and gain units need checking |
| Recording identity | OpenSubsonic MusicBrainz/ISRC and Jellyfin ProviderIds | Current normalized model discards them; matching remains fallible |
| Strong artist links | Contributor data and MusicBrainz relationships | Actual collection coverage, caching, confidence, fallback behavior |

OpenSubsonic distinguishes now-playing notifications from completed scrobbles. Its specification prohibits stream access itself from increasing playcount; verify older/classic servers separately. Withholding a completed scrobble is preferable to trying to undo history. Subsonic rating 0 removes a rating; it is not dislike. Never map unstar or one-star ratings silently to a universal boolean dislike.

Jellyfin gain/annotation findings refer to current upstream API source, not an observed installed version. MusicBrainz lookups need a proper User-Agent, rate limiting, and caching. A relationship metadata cache is factual library information, not a user taste profile.

Primary references: [OpenSubsonic stream](https://opensubsonic.netlify.app/docs/endpoints/stream/), [transcode offset](https://opensubsonic.netlify.app/docs/extensions/transcodeoffset/), [scrobble](https://opensubsonic.netlify.app/docs/endpoints/scrobble/), [ratings](https://opensubsonic.netlify.app/docs/endpoints/setrating/), [ReplayGain](https://opensubsonic.netlify.app/docs/responses/replaygain/), [Child metadata](https://opensubsonic.netlify.app/docs/responses/child/), [Jellyfin audio API source](https://github.com/jellyfin/jellyfin/blob/master/Jellyfin.Api/Controllers/AudioController.cs), [Jellyfin annotations](https://github.com/jellyfin/jellyfin/blob/master/Jellyfin.Api/Controllers/UserLibraryController.cs), [Jellyfin DTO](https://github.com/jellyfin/jellyfin/blob/master/MediaBrowser.Model/Dto/BaseItemDto.cs), [MusicBrainz API](https://musicbrainz.org/doc/MusicBrainz_API).

## Test environment and results

Local host: macOS arm64; Rust 1.98.0; FFmpeg available. Synthetic fixtures generated for WAV, FLAC, ALAC/M4A, MP3, AAC/M4A, and Opus. This FFmpeg build lacks libvorbis; the fixture generator reports that variant as unavailable.

Pinned experiment dependencies: CPAL 0.16.0, Symphonia 0.5.5. The earlier documentation assessment consulted newer versions too; the following measurements apply specifically to these pins. CPAL 0.16's Linux default uses ALSA, so shared desktop routing needs explicit validation before selecting it for production.

Native Mac build passed; nine Rust tests passed, covering malformed/truncated input, direct/hard-link overwrite protection, incompatible formats, callback starvation accounting, continuity between queued frames, and pause behavior. Three Python regression tests passed for encoder timeout reporting, missing-probe reporting, and timeout partial-output preservation. Clippy passed.

### Decoder continuity

The synthetic reference contains 288,041 stereo frames at 48 kHz across three independently encoded files.

| Format | Decoded frames | Comparison | Result |
|---|---:|---|---|
| WAV | 288,041 | Exact original samples, including boundaries | Pass |
| FLAC | 288,041 | Exact original samples, including boundaries | Pass |
| ALAC/M4A | 288,041 | Exact original samples, including boundaries | Pass for these files |
| MP3 | 288,041 | RMS error 0.00265; boundary RMS about 0.00306 | Pass against fixture-specific 0.02 tolerance |
| AAC/M4A | 292,864 | 4,823 extra frames; RMS error 0.10947 | Fails continuity requirement |
| Opus | — | Decoder reports unsupported codec | Unsupported by this backend |
| Vorbis | — | Installed FFmpeg lacks libvorbis encoder | Not run |

These results do not certify all files in a format or physical gapless output. ALAC passing these fixtures refines the documentation concern: its container support must be tested on representative files rather than presumed universally broken. AAC adds roughly 100.5 ms total excess at 48 kHz in this test. A production backend choice must address AAC trimming and Opus support; these are measured limitations, not acceptance-test failures to hide.

### Native CoreAudio callback

The sandbox exposed no endpoints. Approved out-of-sandbox enumeration exposed CoreAudio devices. A silent run on the MacBook Pro speakers (`--volume 0`) consumed all 288,041 frames with **zero reported underrun frames**. No system audio settings were changed. The process used 6.48 seconds wall time, 0.23 seconds user CPU, 0.05 seconds system CPU, and maximum RSS 25,460,736 bytes (about 24.3 MiB), measured by macOS `/usr/bin/time -l` on the debug build. This is the standalone probe's process memory, not incremental production daemon cost or a low-memory release guarantee.

A second silent run alongside synthetic local I/O consumed the same frame count with **zero reported underrun frames** in 6.83 seconds. The writer submitted 7,877,951,488 bytes through repeated writes to a bounded 16 MiB scratch file, with flush/fsync and pacing. This is synthetic scheduling pressure; caching and repeated-file behavior mean it is not a measurement of portable-device throughput or actual sync performance.

Both runs verify callback execution and observed starvation counters only. Physical sound was not captured or listened to. The probe pre-decodes the finite inputs for validation, then decodes again into a bounded queue. It does not prove streaming startup latency. Its fixed 250 ms drain is experimental and cannot guarantee complete physical output on every endpoint.

After review corrected a potential starvation-counter race, the synthetic-I/O run was repeated: 288,041 frames consumed, zero underruns, 6.88 seconds, 7,885,291,520 submitted scratch-write bytes. The original decoder outcomes were reproduced with additional per-track metadata checks. A separate silent stdin pause/resume/stop run stopped at 19,456 consumed frames with zero underruns. This tests basic transport commands; it is not a keyboard media-key test.

### Review and hardening

Independent adversarial, edge-case, and acceptance reviews identified output hard-link destruction, premature EOF acceptance, an underrun-counting race, errors ignored during final drain, unbounded worker cleanup, unsupported channel-layout acceptance, timeout reports that could leave stale success files, and missing basic transport controls. These were addressed with exclusive output creation, declared-length checks, corrected counters, drain error checks, cancellation plus a whole-process deadline, standard-stereo scope, explicit failure reports, and stdin transport. No production files were changed.

### Still untested

Windows/Linux runtime, USB unplug, audible/loopback gapless output, real sync, OS media keys, production UI lifecycle, live-provider bandwidth adaptation, and actual metadata coverage remain untested. Repository examples and mock tests are not evidence of a running test server. No credential stores were inspected.

The next technical decision should compare a backend/decoder combination that covers the required AAC and Opus cases, then implement the durable user-session lifecycle and media-key proof on all three OSes. The existing experiment is a repeatable baseline for that comparison, not a shipping playback engine.

## FFmpeg decoder comparison

The same verifier now supports an optional FFmpeg CLI adapter. On this Mac, **FFmpeg 9.0.1** passed all six available formats with identical tolerances and exactly **288,041 frames**, including exact per-track lengths. No manifest-driven trimming or explicit resampling was applied.

| Format | Full-signal RMS error | Boundary RMS errors | Result |
|---|---:|---|---|
| WAV / FLAC / ALAC | 0 | 0 / 0 | Exact PCM match |
| MP3 | 0.002653 | 0.003058 / 0.003051 | Pass |
| AAC | 0.002406 | 0.003049 / 0.002934 | Pass |
| Opus | 0.000944 | 0.001696 / 0.001169 | Pass |

Vorbis remains not run because the fixture encoder is unavailable. The Symphonia comparison was repeated and retained its AAC padding and unsupported Opus outcomes. These are short synthetic fixtures; measured RMS thresholds are not perceptual quality certification. Reports retain executable version and build configuration in ignored `results/ffmpeg-comparison.json` and `results/symphonia-comparison.json`; the commands in the experiment README reproduce them.

### Integration recommendation

FFmpeg is a credible candidate for the next Rust decoding experiment. The CLI result does **not** prove that a Rust binding automatically reproduces container trimming, decoder draining, timestamps, or Opus pre-skip. A worker using libavformat/libavcodec and, when needed, libswresample should reproduce this exact matrix before connection to the native audio callback. Keep FFmpeg-owned objects on that worker and transfer owned PCM through the bounded queue.

The [ffmpeg-next wrapper](https://github.com/zmwangx/rust-ffmpeg) is in maintenance mode. Version 9 is a candidate, but its upstream [unreleased changelog](https://raw.githubusercontent.com/zmwangx/rust-ffmpeg/master/CHANGELOG.md) reports fixes to unsound Send implementations involving shared contexts. Establish which fixes a pinned release contains before adopting it. Thread confinement reduces exposure but does not replace that check.

The [build documentation](https://github.com/zmwangx/rust-ffmpeg/wiki/Notes-on-building) describes native headers/libraries and platform setup. Shipping would require pinned matching libraries, Windows DLL/import-library packaging, macOS relocation and signing, and a defined Linux runtime baseline. Test installation and playback on clean machines for each supported OS and architecture. This CLI test has exercised none of that packaging.

The local Homebrew binary has `--enable-gpl --enable-version3`; it is an experiment tool, not a selected distribution build. FFmpeg's [licensing documentation](https://ffmpeg.org/legal.html) explains how enabled components affect distribution obligations. Choose and document an intentional audio build if FFmpeg is adopted.

This removes the observed AAC/Opus decoder obstacle for these files. Native FFmpeg playback, physical gapless transitions, Windows/Linux runtime, media keys, durable daemon lifetime, USB disconnects, live sync contention, and server quality adaptation remain pending.

## Native Rust FFmpeg proof

Executed on macOS 26.6.2 (25G83), arm64, using optional `native-ffmpeg` feature in the standalone probe. Pinned `ffmpeg-next` and `ffmpeg-sys-next` are 9.0.0. Runtime identity and `otool -L` confirm direct linkage to libavcodec 63.1.101, libavformat 63.1.101, and libavutil 61.1.101 from the installed Homebrew FFmpeg build. The playback process launches no CLI decoder or Python process.

### Decoding and audio callback

All six generated formats pass the same verifier with exactly 288,041 total frames and exact individual track lengths. WAV/FLAC/ALAC match PCM exactly; MP3, AAC and Opus have the same RMS and boundary results as the CLI table above. FFmpeg's decoder applies available codec/container skip metadata itself; this implementation adds no duration clamp, fixture-length trimming, resampling, or silence. It drains the decoder, converts native sample representation to interleaved f32, then forwards PCM into the existing bounded queue.

The first run rejected untagged stereo WAV layout. The corrected backend accepts an unspecified layout only when it has exactly two channels, interpreted as conventional stereo, while rejecting explicitly different channel layouts such as stereo-downmix. It also restricts demuxer protocols to local files. The corrected full six-format matrix passes.

| Silent native sequence | Consumed frames | Underrun frames | Wall / user / system seconds | Maximum RSS |
|---|---:|---:|---|---:|
| AAC, three tracks | 288,041 | 0 | 6.81 / 0.30 / 0.04 | 29,605,888 bytes (28.2 MiB) |
| Opus, three tracks | 288,041 | 0 | 6.78 / 0.36 / 0.05 | 30,277,632 bytes (28.9 MiB) |

Both runs explicitly selected `Haut-parleurs MacBook Pro` at volume zero, 48 kHz stereo. These timings and memory figures are the standalone debug process, including preflight; they are not production overhead, startup latency, or release performance guarantees. They exercise CoreAudio callbacks but do not measure audible/physical gaplessness, shared mixing with another application, output unplug, or concurrent real sync. The existing 250 ms final drain remains experimental.

Reports are generated locally under `experiments/playback-probe/results/native-ffmpeg.json`, `native-ffmpeg-aac-play.json`, and `native-ffmpeg-opus-play.json`. They preserve commands, backend/library identity, counters and resource output. The README provides repeatable build and run commands.

### Isolation and remaining work

The default build was rebuilt and inspected: no libavcodec/libavformat/libavutil linkage, explicit failure when requesting the disabled native backend, and the same Symphonia codec outcomes as before. Production manifests and sources remain unchanged. Native tests cover malformed/truncated input, conversion/interleaving, untagged stereo, explicit unsupported layout, nonfinite samples and the existing callback behavior.

The released wrapper still contains the `Rc`/unsafe Send concern identified in its [context source](https://docs.rs/ffmpeg-next/9.0.0/src/ffmpeg_next/codec/context.rs.html). All FFmpeg contexts are created, used and destroyed within one invocation on the creating thread; preflight and playback create separate contexts. Only PCM crosses into the queue. This bounds the experiment's exposure but does not certify the wrapper for production threading.

Rust plus FFmpeg plus native output is therefore a viable candidate on this Mac for the tested files. Windows WASAPI and Linux desktop audio runtime, clean-machine packaging on every target, physical gapless capture, media keys, durable user-session daemon lifecycle, and real server buffering/adaptation remain untested. Vorbis remains not run because the fixture encoder is absent. The next proof should focus on user-session lifecycle/media controls and repeat native output on Windows/Linux, while evaluating a pinned wrapper/build suitable for distribution.

### Native proof review and regression checks

Independent general, edge-case and acceptance reviews found three corrections: restrict nested protocols, select the first audio stream consistently with the CLI adapter, and reject RIFF/WAVE truncation even when FFmpeg reaches a clean packet boundary. The packet-boundary defect was reproduced with a 32,768-byte declared payload containing only 16,384 bytes; FFmpeg otherwise returned success. The backend now validates RIFF and chunk bounds before decoding. RF64 and other containers still rely on FFmpeg's own detection; this is not exhaustive corruption validation.

After correction, 14 native Rust tests, 9 default Rust tests and both nine-test Python suites pass; native Clippy is clean. The six-format matrix was repeated successfully. An additional two-stream container with its second stream marked default still selected the first stream and matched reference PCM exactly. A generated HLS manifest referencing HTTP failed with the explicit file-only protocol whitelist error. Follow-up edge review found no remaining actionable defects.
