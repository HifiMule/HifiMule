# Investigation: M4B device profiles

## Hand-off Brief

1. **Request.** Evaluate M4B eligibility for all nine bundled profiles and identify useful new profiles.
2. **State.** Evaluation complete: Garmin and compatible Rockbox targets have documented M4B support; PocketBook and Android/Voice are evidence-backed new candidates. The user prefers splitting broad profiles into versions with and without audiobook support.
3. **Next.** Implement explicit M4B-preservation support and a Modern DAP profile split, with migration and focused sync/transport checks; the profile JSON alone is insufficient.

## Case Info

| Field | Value |
| --- | --- |
| Date opened | 2026-10-22 |
| Status | Concluded: evaluation complete, implementation not performed |
| Scope | Bundled profiles; Audiobookshelf download and device sync compatibility |
| Evidence sources | Current working tree, official device documentation, upstream project documentation |

## Problem Statement

User request: “With audibookshelf we now manage M4B files for audiobooks and podcasts. But existing profiles doesn't have information about m4b support. Evaluate for each existing profile if m4b can be added, and find if we can create new profiles integrating m4b support.”

This is an evaluation. Existing staged code and profile edits are inputs and remain untouched.

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| Bundled profile JSON | Available | Nine profiles; no explicit m4b DirectPlayProfiles entry |
| Sync/provider/transfer source | Available | Source trace completed; citations below |
| Official compatibility documentation | Available with limits | Garmin/Rockbox/PocketBook/Voice explicit; Sony evidence is model-specific; no universal modern-DAP guarantee |
| Physical device playback | Missing | No device validation performed |

## Investigation Backlog

| Path | Priority | Status |
| --- | --- | --- |
| All nine existing profiles | High | Done |
| Metadata, compatibility matching, extension, scan, MTP, seeding | High | Done: implementation gaps recorded |
| New profile candidates | Medium | Done |
| Physical device checks | High before release | Requires actual devices |

## Evaluation of every existing profile

“Add M4B” below means permit original `.m4b` files on the device, not merely accept M4B as a source for conversion. All decisions concern unencrypted media with a verified supported audio codec. A device decoding AAC does not establish that its library scanner accepts the `.m4b` extension, or that its UI supports chapters and bookmarks.

| Profile ID | Can M4B be added? | Recommended policy | Evidence / confidence |
| --- | --- | --- | --- |
| `passthrough` | Already permits originals by design; no direct-play list to extend. | Explain that originals include M4B and compatibility remains the user's responsibility. Correct the Jellyfin-only description. Preserve the original extension. | JSON:4; confirmed profile policy, device support unspecified. |
| `rockbox-mp3-320` | Yes for compatible AAC M4B, with target qualifications. | A reasonable candidate to extend after the common pipeline fixes. Prefer AAC-LC for the broad Rockbox target; retain MP3 fallback and existing bitrate limit. Do not advertise all AAC variants or embedded chapter navigation. | JSON:10; [official Rockbox manual source](https://raw.githubusercontent.com/Rockbox/rockbox/master/manual/appendix/file_formats.tex), lines 85–90, 125–126; documented format support. |
| `rockbox-mp3-192` | Technically yes on the same compatible Rockbox targets. | Keep the existing MP3-oriented storage policy for now. Direct M4B at or below 192 kbps is possible, but changes the documented “other audio to MP3” behavior. Prefer an explicit audiobook preset rather than silently changing this policy. | JSON:39; same Rockbox evidence; recommendation is a policy judgment. |
| `generic-mp3-player` | No blanket native M4B claim. | Continue converting M4B sources to MP3 256 kbps. | JSON:66; MP3-only target contract. |
| `garmin-music` | Yes, strongest existing candidate, AAC only. | Explicit M4B/AAC direct copy within existing 320 kbps, two-channel, 48 kHz limits; MP3 fallback. Exclude ALAC and unsupported AAC variants; validate actual watch ingestion. | JSON:92; [Garmin's format support article](https://support.garmin.com/it-IT/?faq=JyNEOTsZaR3KMXqej3oQp5) explicitly lists M4B and excludes ALAC. |
| `modern-dap-lossless` | Yes as a split, not a blanket extension of the category. | Retain this ID's current music/lossless capability set and add a sibling explicitly supporting M4B for compatible devices/apps. Do not infer ALAC-in-M4B support from ALAC-in-M4A support. | JSON:121; user explicitly accepts a two-profile split. No manufacturer guarantee for the whole category was found. |
| `sony-walkman-aac-flac` | Do not add native M4B to the broad profile. | Continue AAC/M4A output. A future tested lossless copy/remux from AAC/M4B to `.m4a` could avoid re-encoding, but is not native M4B support. Android Walkmans using a separate compatible audiobook app belong under that app's profile. | JSON:157; [Sony NW-A55 family specification](https://helpguide.sony.net/dmp/nwa50/v1/en/contents/TP0002342895.html) lists AAC `.mp4/.m4a/.3gp`, omits `.m4b`. Absence is grounds to withhold the promise, not proof every Sony model rejects it. |
| `car-usb-mp3` | No blanket native M4B claim. | Preserve mandatory MP3 256 kbps conversion. A particular head unit needs its own documented/tested profile. | JSON:189; deliberately empty direct-play list. |
| `spoken-word-mp3-96` | M4B input yes; original M4B output would change its purpose. | Keep compact MP3 96 kbps conversion. Offer a separate preservation preset for users who prefer original M4B. | JSON:216; deliberately empty direct-play list. |

The Garmin article is JavaScript-rendered: the research search index returned its full English body under the Italian locale, while opening the page returned only the shell. Its [Japanese locale](https://support.garmin.com/ja-JP/?faq=JyNEOTsZaR3KMXqej3oQp5) also exposes a model-specific Fenix 5 Plus M4B sync caveat in indexed text. Treat current status of that historical caveat as unverified and include an affected model in hardware testing if available.

For Audiobookshelf, conversion fallbacks in this table require **local encoding enabled for the source server**. Without it, an incompatible AAC original is skipped; Audiobookshelf does not supply the server-side file transcoding route needed to honor these output targets. Thus “MP3 fallback” describes a supported configured conversion path, not automatic server conversion.

## New profile candidates

These are proposed designs, not implemented profiles or hardware certifications. Folder names below are proposed HifiMule defaults, not manufacturer requirements. Use `Audiobooks` for media and `Playlists` only where external playlists are verified useful.

| Proposed ID / name | Direct-copy formats | Conversion fallback | Priority and qualification |
| --- | --- | --- | --- |
| `modern-dap-lossless-m4b` — Modern DAP — Lossless + M4B Audiobooks | Existing modern-DAP formats plus explicit M4B/AAC. Do not add M4B/ALAC until supported by provider and target evidence. | Retain FLAC fallback and existing ceilings if the promise is the same lossless profile plus M4B support. | Recommended split requested by user. Keep `modern-dap-lossless` as the version without explicit M4B support. This selection asserts the user has a compatible device/app; it does not certify every modern DAP. |
| `audiobooks-m4b-mp3` — Audiobooks / Podcasts — Preserve M4B | Verified AAC-LC in M4B/M4A and MP3. Preserve compatible originals; do not use a 96 kbps direct-copy ceiling if preservation is the promise. | MP3 96 kbps, at most stereo/44.1 kHz; 96 kbps is a proposed speech preset, not a documented device limit. | Highest-value general policy preset, explicitly for devices/apps known to accept M4B. Covers a need distinct from the existing force-MP3 preset. No broad hardware claim. |
| `pocketbook-audiobooks` — PocketBook — Audiobooks | AAC M4B/M4A, MP3, Ogg/Vorbis; start with verified AAC-LC. Do not infer ALAC, Opus, arbitrary AAC variants, or ZIP transfer support from the extension list. | MP3 96 kbps proposed; confirm model-specific limits. | Strongest new hardware-family candidate. PocketBook explicitly documents M4B in its Audiobooks application and USB file copying. Limit to audiobook-capable models. [Official FAQ](https://pocketbook.ch/en-ch/faq), [Color 633 manual, Audiobooks section](https://support.pocketbook-int.com/fw/633/u/633.6.8.2817/manual/ww/User_Manual_Color_EN.pdf). |
| `android-voice-audiobooks` — Android / Voice — Audiobooks | AAC M4B/M4A, MP3, Ogg/Vorbis and Opus, subject to device decoder support. | MP3 96 kbps proposed, or original-only if the user explicitly chooses it. | Strong app-qualified candidate for Android phones/DAPs. Voice documents local M4B files, chapter navigation, bookmarks, and resume. USB/MTP or an SD card must expose the chosen audiobook folder; grant the app access. [Official Voice repository](https://github.com/PaulWoitaschek/Voice). |

Avoid creating a duplicate Garmin audiobook profile unless a distinct compact-output policy is desired; extend its existing profile instead. With the user's preference for explicit choices, `rockbox-audiobooks-m4b` is a useful optional sibling to the two MP3 presets: direct-copy verified supported AAC M4B and MP3, with MP3 96 kbps fallback. Keep the original two presets if preserving their current policy is preferred. Do not split every preset mechanically: an audiobook label cannot give a basic MP3 player native M4B capability.

The existing `spoken-word-mp3-96` and proposed `audiobooks-m4b-mp3` naturally form the same two choices: compact MP3 output versus M4B preservation. The “without audiobook support” label should be avoided in the final UI: MP3/M4A profiles can still play spoken-word content; the actual distinction is **M4B direct support**.

An “AAC 64/96 kbps M4B output” preset is a different feature: it creates new M4B files rather than preserving existing ones. Defer until M4B muxing/filename handling, chapter preservation, and multi-file behavior are implemented and verified. A profile JSON entry alone must not advertise book merging or chapter authoring.

Do not add a stock Apple iPod profile based on its audiobook codec support. [Apple's guide](https://support.apple.com/en-ie/guide/itunes/itns3114/windows) distinguishes ordinary disk copying from synced media; the [iPod classic user guide](https://cdsassets.apple.com/live/6GJYWVAV/user/ma1195_ipod_classic_160gb_user_guide.pdf) directs audiobook transfers through iTunes. Deduced: a format preset alone does not implement that media-library integration. Rockbox iPods remain a separate supported target.

## Format and capability boundaries

Use separate decisions for original extension recognition, decoded audio compatibility, embedded chapter preservation, device chapter navigation, and playback position handling. A profile can select a file format but cannot establish all five behaviors. DRM-protected files are outside these compatibility promises.

AAC-LC is the conservative starting point. Rockbox's manual makes HE-AAC conditional on the build/target. [Audiobookshelf's own FAQ](https://audiobookshelf.org/docs/faq/server/) documents xHE-AAC parsing/decoder limitations dependent on its FFmpeg installation. “AAC” metadata alone must not be presented as proof that every AAC variant is supported.

## Hypothesized Paths

### Hypothesis 1: Profiles omit explicit M4B capability

**Status:** Confirmed.

**Evidence:** hifimule-daemon/assets/device-profiles.json has nine entries and no m4b container declaration. This does not prove M4B cannot already pass through some runtime paths.

### Hypothesis 2: Adding M4B to the profile JSON is sufficient

**Status:** Refuted.

**Would confirm:** Correct codec-aware direct-copy selection, preserved output extension, device discovery, transfer, and chapter behavior across representative M4B sources.

**Would refute:** Any missing handling in those paths.

**Resolution:** Confirmed source normalization, missing local M4B output handling, scanner/transport gaps, and non-updating profile arrays demonstrate that a JSON-only change does not deliver reliable native M4B support.

## Confirmed Findings and Source Code Trace

| Area | Evidence | Consequence |
| --- | --- | --- |
| Original type normalized away | `hifimule-daemon/src/providers/audiobookshelf.rs:1189`, `:1191`, `:3666`, `:3668`; `hifimule-daemon/src/sync.rs:1855`, `:3952` | Verified `audio/mp4` + `aac` becomes `m4a` for books and podcasts; destination naming follows that representation. Original M4B may already direct-copy as M4A under AAC profiles. Preserve original filename extension separately from MP4 family identity. |
| Supported ABS source combinations | `hifimule-daemon/src/providers/audiobookshelf.rs:1189`, `:3666`, `:3670` | Only verified MPEG/MP3 and MP4/AAC are admitted here. ALAC-in-M4B is rejected before profile selection; adding an ALAC profile row cannot enable it. |
| Container matching | `hifimule-daemon/src/sync.rs:1431`, `:1937`, `:1945`, `:2015` | MP4/M4A are aliased; M4B is not. MIME can also contribute family keys. A new extension check must be separate from codec/container-family compatibility; globally aliasing M4B risks authorizing unsupported extensions under Sony and other existing AAC profiles. |
| AAC variant limits | `hifimule-daemon/src/domain/models.rs:195`; `hifimule-daemon/src/providers/mod.rs:1024`; `hifimule-daemon/src/sync.rs:1497`, `:1506` | Current source/profile matching lacks AAC-LC/HE/xHE evidence/enforcement. An `AudioCodec: aac` row cannot enforce LC-only support. |
| Local encoding target | `hifimule-daemon/src/transcoding/local.rs:79`, `:81`, `:92`, `:188`, `:207` | M4A/IPOD and MP4/MOV targets exist; M4B output is rejected. Nonempty AudioProfile/CodecProfiles conditions are also rejected, so adding those conditions to JSON is not a complete LC enforcement solution. |
| Fallback execution | `hifimule-daemon/src/providers/audiobookshelf.rs:3801`; `hifimule-daemon/src/sync.rs:3552`, `:3594`, `:3629` | ABS has no server-file-transcoding route here. Enable local encoding for incompatible sources, otherwise sync skips them. |
| Direct tagging chapters | `hifimule-daemon/src/sync_tags.rs:36`, `:759`, `:786`, `:804` | Existing M4B tagging implementation and tests cover chapter lists/tracks, artwork and unchanged audio. Tests were inspected, not executed in this evaluation. |
| Conversion chapters | `hifimule-daemon/src/transcoding/local.rs:810`, `:817`, `:827`, `:883`; `hifimule-daemon/src/sync.rs:4238`, `:4324` | Local encoding copies metadata/artwork, but inspected code has no chapter-copy operation. Deduced: do not promise chapters survive re-encoding. |
| Multi-file books | `hifimule-daemon/src/providers/audiobookshelf.rs:2726`, `:2730`, `:2789`; `hifimule-daemon/src/providers/mod.rs:187` | One track per source audio file; chapters are book metadata, absent from SyncBookTags. Profile entries do not merge books or split chapters. |
| Device discovery / transfer | `hifimule-daemon/src/device_music_scan.rs:67`, `:501`; `hifimule-daemon/src/device/mtp.rs:1669`, `:1682` | Music scanner omits M4B; libmtp maps it to UNKNOWN. Determine appropriate audio/audiobook transfer type and verify device indexing. Music scanning alone does not establish audiobook reconciliation. Windows MTP requires its own check. |
| Existing installations | `hifimule-daemon/src/transcoding.rs:36`, `:90`, `:100`, `:134`, `:153` | Seeding appends new profile IDs but preserves existing arrays. Shipping added M4B rows does not update existing DirectPlayProfiles arrays. Use a migration preserving user edits or introduce new IDs. |

## Deduced Conclusions

1. M4B support has three independent implementation concerns: source identity, target capability, and audiobook features. The container/profile entry alone covers only part of target capability.
2. New sibling profile IDs suit the user's desired split and the existing seeding behavior. Existing ID migration is still required wherever the old preset itself is changed, such as Garmin.
3. Preserving existing M4B is a smaller first feature than creating M4B output. Direct-copy tagging already has chapter-preservation coverage, whereas encoding and book authoring require additional work.

## Missing Evidence

| Gap | Impact | How to obtain |
| --- | --- | --- |
| Specific DAP/watch model and firmware validation | Documentation is not an end-to-end HifiMule certification | Copy original long AAC-LC M4B through actual transport; play, seek, resume, inspect chapters |
| AAC variant metadata and unknown handling | LC-only recommendations cannot currently be enforced | Probe source variant, carry it through compatibility selection, test LC/HE/xHE and unknown |
| ALAC source acceptance | Lossless M4B cannot currently flow from ABS through this source resolver | Add explicit supported source mapping and test codec-aware target decisions |
| Transcoded chapters | No preservation guarantee | Inspect output chapter atoms/tracks after local conversion |
| M4B discovery and MTP indexing | A transfer may not appear in the device library | Verify filesystem/libmtp/Windows MTP and intended audiobook reconciliation |
| Current status of Garmin Fenix caveat | Older-model limitations may remain | Test affected model/firmware or obtain current manufacturer clarification |

## Timeline of Events

| Date | Event | Confidence |
| --- | --- | --- |
| 2026-10-22 | Enumerated all nine bundled profiles against current staged working tree | Confirmed |
| 2026-10-22 | Researched Rockbox, Garmin, Sony, PocketBook, Voice and ABS primary documentation | Confirmed, with fetch/index limitations noted above |
| 2026-10-22 | User accepted splitting Modern DAP and other broad presets into two versions | Confirmed user preference |
| 2026-10-22 | Completed source trace; JSON-only sufficiency hypothesis refuted | Confirmed |

## Conclusion

**Confidence:** High for the profile inventory and source limitations; medium for deployment eligibility until hardware testing.

Recommend the Modern DAP split, explicit Garmin support, and an optional Rockbox audiobook sibling. PocketBook and Android/Voice are defensible new targets. Keep generic MP3, car USB and compact MP3 presets' format contracts. Sony remains M4A-oriented unless a particular app/model is documented and tested for M4B. New profiles must be presented as conditional capabilities, not proof of chapter/resume or codec-variant support.

## Recommended Next Steps

1. Implement native source-extension preservation and explicit per-target M4B acceptance, without globally granting M4B to every MP4/M4A profile.
2. Add `modern-dap-lossless-m4b` as the sibling requested by the user. Keep the existing ID and settings for devices without native M4B recognition. Decide whether the general preservation preset or the Rockbox-qualified sibling is useful enough to ship; avoid duplicate settings with indistinguishable promises.
3. Extend Garmin with safe migration, and add PocketBook/Voice profiles when transport checks pass. Preserve local-encoding requirements in descriptions/documentation.
4. Add M4B scanning/transfer handling and AAC-variant enforcement as required to support the advertised device restrictions.
5. Defer M4B encoding, chapter authoring/merging, and ALAC claims until their distinct paths are implemented and verified.

Implementation workflow: `bmad-quick-dev`; use a tracked spec/story if the source-property and compatibility changes need to be split across work items. No implementation or approval is required to complete this evaluation.

## Verification Plan

| Case | Expected behavior |
| --- | --- |
| AAC-LC M4B with chapters, M4B-enabled profile | Preserve `.m4b`, audio payload, artwork and chapter data; verify actual device indexing, seek and resume |
| Same source, Modern DAP profile without M4B | Produce a supported extension/format through an explicit supported path; no accidental grant from MIME aliases |
| Same source, compact MP3 profile | Local encoding enabled: MP3 target; disabled: clear incompatibility/skip, no server-transcoding assumption |
| M4B carrying ALAC | Explicit current unsupported-source outcome; no false promise from lossless profile |
| HE/xHE/unknown AAC variant | Accept only when supported by the target and verified; otherwise reject or use a proven conversion path |
| One long book and one multi-file book | Preserve item boundaries; no implicit merging/splitting; test long-duration seeking and size limits |
| Existing profile file with user edits | New IDs appended; explicit shipped-profile migration preserves custom arrays |
| Filesystem, libmtp, Windows MTP | File arrives, is indexed as intended, and remains correctly managed by sync |

No build, automated tests, or physical device checks were run: this task produced an evaluation document and no production-code changes. Existing staged changes were left intact.

## Follow-up: 2026-10-23 — implementation

The user approved implementation. `spec-m4b-device-profiles.md` records the completed change: five additional profiles, Garmin support and migration, preserved M4B sync identity, explicit compatibility checks, distinct direct/conversion bitrate policies, and scanner/libmtp handling. Existing music profiles remain available.

Eight M4B regression tests passed. The final broader run passed 1,666 daemon tests and five integration contract tests, with 12 ignored and only two pre-existing Windows path-policy failures excluded after reproducing both on baseline `4b9b886`. Independent reviews were completed and their introduced-policy findings fixed with observed failing regressions first.

AAC-variant probing, ALAC source admission, M4B encoding/merging/splitting, conversion chapter preservation and hardware certification remain outside the implemented scope, as specified before coding.
