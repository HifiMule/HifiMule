# Investigation: Navidrome seeking unavailable on macOS

## Hand-off Brief

1. The user reports the unqualified-source message for every tested Navidrome format on macOS, including MP3, M4A and Opus.
2. The source confirms a blanket provider gate: Subsonic/Navidrome declares no seek mechanism and disables byte-range support; the audio engine only recognizes the Jellyfin PCM-WAV candidate.
3. Supporting this setup requires expanding the provider/format implementation and qualification scope, not retesting unchanged controls or changing the message.

## Case Info

| Field | Value |
| --- | --- |
| Ticket | N/A |
| Date opened | 2026-09-18 |
| Status | Active — scope and stronghold established |
| System | User-reported macOS with Navidrome; versions and architecture not supplied |
| Evidence sources | User observation; current provider and audio-engine source; story 15.7 review context |

## Problem Statement

User report: “For all format test (including mp3, m4a, opus...) on osx with navidrome, I got the message ‘this source and audio format is not qualified for seek’.”

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| User observation | Available | Same unavailable message across formats |
| Provider source | Available | `hifimule-daemon/src/providers/subsonic.rs:493` sets `seek_mechanism: None`; line 497 sets `range_supported: false` |
| Audio capability gate | Available | `hifimule-daemon/src/playback/audio.rs:692` only recognizes `JellyfinOriginalPcmWav` |
| Installed seek evidence | Missing | Ordinary playback or seeing the disabled message does not establish seek accuracy |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| --- | --- | --- | --- | --- |
| 1 | Explain provider-wide rejection | High | Done | It occurs before per-format seek validation |
| 2 | Expand scope to Navidrome original streams and macOS formats | High | Open | Existing story initially excludes these combinations |
| 3 | Verify authenticated raw-stream ranges and representation identity | High | Open | No server behavior is inferred from the disabled flags |
| 4 | Validate MP3/M4A/Opus demux seeking, delay, duration and resampling | High | Open | Keep existing sequential-probe safeguards |
| 5 | Collect installed accuracy, paused-seek and output/race evidence | High | Blocked | Requires candidate implementation and real setup |

## Confirmed Findings

### Finding 1: Navidrome is excluded before format qualification

**Evidence:** `hifimule-daemon/src/providers/subsonic.rs:472`, `:493`, `:497`; `hifimule-daemon/src/playback/audio.rs:692`.

**Detail:** Every resolved raw representation uses `seek_mechanism: None` and `range_supported: false`. The engine's candidate predicate requires the Jellyfin-specific mechanism plus range support. MP3, M4A, Opus and WAV from this adapter all fail that predicate.

## Deduced Conclusions

The reported message is expected under the current code policy. It does not demonstrate an inherent limitation of Navidrome or of the tested formats. The initial implementation excluded this provider; the review additionally closed runtime enablement for unqualified installed combinations. The review fixes did not deliver usable seeking on the user's setup.

## Hypothesized Paths

### Hypothesis 1: The tested files individually failed seek validation

**Status:** Refuted as the explanation for this message in the current source.

**Resolution:** The provider-wide predicate excludes them before file-level seeking is attempted.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | --- | --- |
| Navidrome/macOS versions and CPU architecture | Defines the qualification row | Record during installed verification |
| Raw-stream range and validator behavior | Determines bounded random access | Controlled authenticated HTTP tests |
| Decoded and audible landing accuracy per codec/container | Determines supported combinations | Independent fixtures and installed tests |

## Source Code Trace

| Element | Detail |
| --- | --- |
| Trigger | Resolve and start a Navidrome track |
| Condition | No mechanism and no advertised range support |
| Gate | Jellyfin-specific candidate test in `audio.rs:692` |
| Outcome | Unavailable representation capability shown by playback controls |

## Conclusion

**Confidence:** High for the source-level explanation; the installed binary was not independently inspected.

The current implementation cannot enable Navidrome seeking in any format. Broader support is implementation and qualification work, not a successful format test that can be recorded as evidence.

## Recommended Next Steps

Expand the story scope to the user's macOS/Navidrome setup, implement provider-neutral verified seeking, and qualify MP3/M4A/Opus and other requested combinations individually. Do not replace the gate with an unconditional success flag.

## Reproduction Plan

Resolve any Navidrome original track and inspect the resulting capability. The current provider flags necessarily fail the candidate predicate, independently of codec. After implementation, test forward/backward and paused seeks against known media-time fixtures and record actual landing and transport outcomes on the installed setup.
