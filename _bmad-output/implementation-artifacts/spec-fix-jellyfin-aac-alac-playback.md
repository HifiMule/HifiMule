---
title: 'Fix Jellyfin AAC and ALAC playback admission for MP4 container aliases'
type: 'bugfix'
created: '2026-10-24'
status: 'done'
baseline_commit: '4bd49712963b23a9963330092897d52206c4be02'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** AAC and ALAC tracks from Jellyfin can fail playback with an unsupported-format error even though Navidrome playback and sync reencoding work. Both attached samples report `MediaSources[0].Container: mov` and the item-level alias list `mov,mp4,m4a,3gp,3g2,mj2`. HifiMule copies this container into its playback codec field and rejects it before downloading or decoding audio.

**Approach:** Canonicalize known MP4-family container aliases to `m4a` when constructing Jellyfin's original playback representation. Use the same canonical label for the representation ID, codec/container hints and existing verified M4A seeking mechanism. Keep the provider's song metadata and sync behavior intact.

## Boundaries & Constraints

**Always:** Use the existing authenticated original download route and FFmpeg decoder. Retain the existing codec/suffix compatibility convention for playback representations. Normalize case and surrounding whitespace while recognizing MP4-family aliases; preserve existing labels for unrelated formats. Keep the representation ID stable for equivalent container aliases.

**Ask First:** Broader changes to source codec metadata, sync compatibility decisions, provider interfaces or transcoding policy.

**Never:** Add server transcoding as a workaround, admit arbitrary unknown codecs globally, infer the container from misleading `.aac`/`.alac` filenames, change Navidrome behavior, or normalize `Song.suffix` globally as part of this fix.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
| --- | --- | --- | --- |
| Supplied AAC shape | Audio codec AAC, source container mov | Original representation uses m4a, passes shared playback selection, offers existing M4A seek mechanism | Existing fetch/decode errors remain applicable |
| Supplied ALAC shape | Audio codec ALAC, source container mov | Same admission behavior; original bytes are preserved | Existing fetch/decode errors remain applicable |
| Item fallback | No source container; item container mov,mp4,m4a,3gp,3g2,mj2 | Same canonical m4a representation | No extra item request |
| Aliases | mov, mp4, m4a, or corresponding mixed-case/whitespace alias list | Equivalent canonical representation identity and decoder hint | Reject unrelated unknown labels as before |
| Existing audio formats | mp3, flac, wav, opus | Existing representation and seek behavior preserved | Existing errors preserved |
| Missing/unknown metadata | No suffix or an unrelated unknown format | Existing unknown-format behavior preserved; unknown codecs are not newly allowed | Shared selection remains authoritative |
| Working sync path | Source song suffix mov / alias list | Song metadata remains unchanged | Existing sync compatibility and reencoding decisions preserved |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/jellyfin.rs:826` — `resolve_playback` constructs an original representation using `song.suffix` for codec, container and ID.
- `hifimule-daemon/src/providers/jellyfin.rs:1201` — `jellyfin_seek_mechanism` already qualifies m4a seeking.
- `hifimule-daemon/src/providers/jellyfin.rs:1307` — raw container-to-song mapping; leave unchanged.
- `hifimule-daemon/src/providers/mod.rs:279` — shared selection rejects representations whose codec is unsupported; existing m4a support is sufficient.
- `hifimule-daemon/src/playback/audio.rs:577` — decoder hint uses representation container; canonical m4a is already supported.
- `hifimule-daemon/src/sync.rs:1445` — MP4 sync compatibility distinguishes recognized suffixes; avoid changing the song suffix here.

## Tasks & Acceptance

**Execution:**

- [x] `hifimule-daemon/src/providers/jellyfin.rs` — add a small playback-only container normalization helper; identify known MP4 aliases in container values without treating every comma-separated value as MP4.
- [x] `hifimule-daemon/src/providers/jellyfin.rs` — apply the canonical value consistently to representation ID, codec, container and seek mechanism while retaining the original song object and request.
- [x] `hifimule-daemon/src/providers/jellyfin.rs` — add regression tests using compact versions of both supplied metadata shapes, mocked item-detail resolution and the shared playback selector. Cover item-level fallback, alias normalization, ordinary formats and unsupported metadata. Assert song suffix and original URL remain unchanged.

**Acceptance Criteria:**

- Given either supplied item shape, when Jellyfin playback is resolved and selected, then an original m4a representation is admitted without requesting server transcoding.
- Given equivalent MP4 container labels, when constructing a representation, then its ID, container hint and seek mechanism agree.
- Given existing source metadata, when resolving playback, then the returned song preserves the suffix used by sync.
- Given unrelated supported or unsupported formats, when selecting playback, then existing admission behavior remains intact.

## Spec Change Log

## Design Notes

The current playback model intentionally accepts suffix-style codec hints including m4a. A playback-only normalization fixes the reported admission failure with one provider-local change. Normalizing the shared song suffix instead would also affect sync direct-copy decisions, which the user reports already work. Retaining the real MediaStream.Codec across provider/domain models is a separate improvement and is unnecessary for this narrow fix.

## Verification

**Commands:**

- `rtk npm run build:daemon -- test -p hifimule-daemon --bin hifimule-daemon providers::jellyfin` — provider regression tests and existing Jellyfin tests pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon --bin hifimule-daemon providers::tests -- --test-threads=1` — shared playback admission tests pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon --bin hifimule-daemon playback::decoder` — existing decoder and M4A seek coverage passes.
- `rtk git diff --check` — no whitespace errors.

**Runtime confirmation:** Play and seek both original samples from Jellyfin after rebuilding. Actual source audio and a connected server are unavailable in this chat, so fixture-based admission checks do not establish live playback success.

**Results (2026-10-24):** 58 Jellyfin tests passed, 34 shared-provider tests passed serially, and 38 decoder tests passed with 6 intentionally ignored external-file diagnostics. The controlled FFmpeg 9.0.2 runtime was verified. The initial sandboxed provider run could not bind HTTP mock servers; rerunning outside the sandbox passed. Two shared cleanup-registry tests failed in parallel and passed in the serial suite. `rtk git diff --check` passed.

**Review:** Blind adversarial, edge-case, and acceptance reviews reported no actionable findings. No source changes outside the Jellyfin provider were required.

## Suggested Review Order

- Normalize the original playback representation while preserving source metadata and authentication.
  [jellyfin.rs:826](../../hifimule-daemon/src/providers/jellyfin.rs#L826)

- Recognize MP4-family aliases without changing unrelated format labels.
  [jellyfin.rs:1202](../../hifimule-daemon/src/providers/jellyfin.rs#L1202)

- Verify both supplied codec shapes and item-level container fallback through shared admission.
  [jellyfin.rs:1543](../../hifimule-daemon/src/providers/jellyfin.rs#L1543)

- Check alias boundaries and existing supported, unknown, and missing metadata behavior.
  [jellyfin.rs:1507](../../hifimule-daemon/src/providers/jellyfin.rs#L1507)
  [jellyfin.rs:1614](../../hifimule-daemon/src/providers/jellyfin.rs#L1614)
