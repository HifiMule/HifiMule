# Investigation: Jellyfin AAC and ALAC playback rejected

## Hand-off Brief

Jellyfin's MP4-family container label is used as a playback codec, causing both supplied samples to be rejected before decoding. The provider now normalizes these labels only in playback representations, with passing regression coverage for both sample shapes. Live playback of the user's actual files remains to be confirmed after rebuilding.

## Case Info

| Field | Value |
| --- | --- |
| Date opened | 2026-10-24 |
| Status | Concluded |
| System | HifiMule, current checkout at 4bd4971 |
| Evidence sources | Two attached Jellyfin item responses; local provider, admission and decoder code |

## Problem Statement

User reports that AAC and ALAC tracks cannot play from Jellyfin with an unsupported-format error, while Navidrome playback and sync reencoding work.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| AAC attachment | Available | `/Users/akartmann/.codex/attachments/0a1b28b3-fa02-447f-bf2e-367dbe23c929/Texte collé.txt`; codec aac, source container mov, item container mov,mp4,m4a,3gp,3g2,mj2 |
| ALAC attachment | Available | `/Users/akartmann/.codex/attachments/bebb5e4e-9cb2-432c-aa47-76d5837890a9/Texte collé.txt`; codec alac, same container labels |
| Source code | Available | Provider mapping and playback representation admission |
| Audio files / runtime logs | Missing | End-to-end decoding on the user's server remains unverified |

## Confirmed Findings

1. `hifimule-daemon/src/providers/jellyfin.rs:1307` copies the first source's container verbatim into `Song.suffix`, falling back to the item container.
2. `hifimule-daemon/src/providers/jellyfin.rs:857` copies that suffix into both representation codec and container.
3. `hifimule-daemon/src/providers/mod.rs:290` filters representations by supported codec. The allowlist at line 341 includes AAC, ALAC, M4A and MP4 but excludes mov and comma-separated format names. An empty result produces `no supported playback representation` at line 308.
4. `hifimule-daemon/src/providers/jellyfin.rs:1201` grants verified MP4 audio seeking only to the m4a suffix.
5. `hifimule-daemon/src/api.rs:118` currently discards MediaStream.Codec, so the sample's real codec does not reach playback admission.

## Deduced Conclusions

Both attached responses map to a representation with codec mov and are rejected before HTTP audio fetch or FFmpeg decoding. A codec installation or forced server transcoding cannot address this metadata rejection. Correcting the provider's interpretation of the format labels addresses the shared cause.

## Hypothesized Paths

| Hypothesis | Status | Resolution |
| --- | --- | --- |
| AAC/ALAC decoder support is missing | Refuted as explanation of this rejection | These representations are removed before the decoder is invoked. Actual audio bytes still require playback verification. |
| Jellyfin container metadata is being mistaken for a codec | Confirmed | The supplied mov value flows unchanged from song mapping into codec admission. |

## Source Code Trace

`get_song` → `song_from_item` → `resolve_playback` → `select_ordered_playback_representations` → unsupported capability when all representations are filtered.

## Investigation Backlog

| Item | Status |
| --- | --- |
| Compare attached metadata with Jellyfin mapping | Done |
| Trace rejection before decoder invocation | Done |
| Define regression coverage and fix direction | Done |
| Verify actual sample audio on user's Jellyfin instance | Requires runtime access |

## Implementation Follow-up: 2026-10-24

- Approved fix implemented in `hifimule-daemon/src/providers/jellyfin.rs:826` and `:1202`.
- MP4-family aliases normalize to m4a for playback ID, codec/container hints and existing M4A seek qualification. Source song metadata and sync compatibility remain unchanged.
- Added provider regression coverage for AAC and ALAC with source mov or item-level aliases; the original route and authorization are preserved.
- Verification: 58 Jellyfin tests, 34 shared-provider tests (serial), and 38 decoder tests passed; 6 external-file diagnostics intentionally ignored. All three independent reviews found no actionable issues.
- Initial mock-server tests required a run outside the sandbox for localhost binding. Two shared cleanup-registry tests interfered in parallel but passed when run serially.

## Conclusion

**Confidence: High** for the metadata/admission defect, deduced deterministically from the supplied responses and source. Live playback is not yet verified.

## Recommended Next Steps

Implement the focused Jellyfin metadata correction, with AAC and ALAC fixtures that pass through provider resolution and shared playback selection. Cover source-level mov and item-level comma-separated fallback, ordinary formats, and unsupported formats. Keep the original download route and verify the normalized M4A representation can use the existing seeking mechanism.

Keep `Song.suffix` unchanged: `hifimule-daemon/src/sync.rs:1457` permits ambiguous MP4 compatibility under AAC/ALAC codec requirements, so globally changing mov to m4a could alter working sync decisions. Canonicalize the playback representation only.
