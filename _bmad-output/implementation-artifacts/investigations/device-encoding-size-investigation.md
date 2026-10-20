# Investigation: Device encoding size estimates

## Hand-off Brief

1. **What happened.** The user supplied a real Jellyfin playlist showing 583 Mo before copying and 545 Mo after copying. Its parent aggregate duration disagrees with the durations of its 91 tracks.
2. **Where the case stands.** Reproduced both rounded numbers at 192 kbps: the basket's parent-duration calculation gives 583 MiB, while daemon per-track planning gives 545 MiB. Confirmed the actual production query returns the same 91 unique tracks, excluding endpoint/pagination differences.
3. **Implementation follow-up.** The current patch shares expanded-track output planning across the basket, preparation and Auto-Fill. Implementation and verification details are recorded at the end of this investigation. This sample is explained by inconsistent playlist duration; FLAC resolution and server caps remain separate concerns.

## Case Info

| Field | Value |
| --- | --- |
| Ticket | N/A |
| Date opened | 2026-10-19 |
| Status | Concluded |
| System | HifiMule working tree, HEAD 85f82dc |
| Evidence sources | Source code, Git history, live playlist metadata and controlled local encoding; copied size remains user-reported |

## Problem Statement

User: “About the recent correction on size estimation depending on the encoding target for a device, do you take into account the original depth and frequency. It seems that for some files the estimation is off.”

## Evidence Inventory

| Source | Status | Notes |
| --- | --- | --- |
| Source code | Available | UI basket and daemon output estimates |
| Version control | Available | a2195e7 introduced device transcoded capacity estimates |
| Representative playlist | Available | Live Jellyfin metadata reproduces the reported rounded estimates; no independent copied-audio measurement |

## Investigation Backlog

| # | Path to Explore | Priority | Status | Notes |
| --- | --- | --- | --- | --- |
| 1 | Source metadata and target encoder | High | Done | Source depth/rate absent from sync model; actual server encoder behavior remains an external evidence gap |
| 2 | UI and sync estimates | High | Done | Compared basket reservation against daemon direct/transcode decisions |
| 3 | Deterministic mismatch example | High | Done | Existing UI helper returns 284,839,936 bytes for 240 seconds at fallback 9216 kbps |
| 4 | Identify user's affected playlist mechanism | High | Done | Parent-versus-child duration reproduces 583-versus-545 MiB at 192 kbps |

## Timeline of Events

| Time | Event | Source | Confidence |
| --- | --- | --- | --- |
| Before HEAD | Device music recognition and transcoded capacity estimates introduced | a2195e7 | Confirmed |

## Confirmed Findings

- `hifimule-ui/src/components/BasketSidebar.ts:185` passes the device's planned bitrate to `plannedSelectionBytes`.
- `hifimule-daemon/src/main.rs:2065` applies `sync::estimate_delta_outputs` with the device transcoding profile.
- `hifimule-ui/src/state/syncCapacity.ts:42` accepts source size, aggregate duration and track count, plus one device target bitrate; there are no depth/rate/channel inputs. The formula at line 44 is duration × target kbps × 125 × 1.03 plus 65,536 bytes per track. Unknown duration reserves one hour per track.
- `hifimule-daemon/src/rpc.rs:4379` supplies `target_bitrate_kbps(profile).unwrap_or(9216)` to the UI.
- Executing the actual TypeScript helper with a 240-second track gives 5,998,336 bytes at 192 kbps and 284,839,936 bytes at 9216 kbps. By arithmetic, 240 seconds of stereo 16-bit/44.1 kHz PCM contains 42,336,000 audio bytes. These are calculated examples, not measured user output files.
- `rtk node --test hifimule-ui/tests/syncCapacity.test.mjs`: 14 passed. Existing tests explicitly require the profile estimate to ignore source bytes. No new product tests or implementation changes were made.
- Delegated source trace: `hifimule-daemon/src/sync.rs:1642` documents the lossless PCM ceiling; `planned_audio_bytes` at line 1645 returns source bytes for compatible direct transfer at line 1649, otherwise uses override/profile bitrate or 9216 kbps at lines 1651–1652.
- Delegated source trace: `hifimule-daemon/src/sync.rs:1706` (`target_bitrate_kbps`) reads MusicStreamingTranscodingBitrate / MaxStreamingBitrate with no FLAC-specific compression model. Thus 9216 is a fallback, not a universal FLAC rule; an explicit bitrate is used unchanged regardless of codec.
- Delegated source trace: `hifimule-daemon/assets/device-profiles.json:125` explicitly configures the Modern DAP profile with 9,216,000 bps and maximum 192 kHz/24-bit/stereo fields.
- Delegated source trace: `hifimule-daemon/src/domain/models.rs:103` (`Song`, through line 134) has source bitrate, duration and size, without source depth/sample rate. Sync transfer records likewise omit these fields.
- Delegated source trace: compatibility at `hifimule-daemon/src/sync.rs:1636` checks source bitrate but does not check sample rate/depth/channel limits. `hifimule-daemon/src/providers/jellyfin.rs:1383` reconstructs the device profile with container/codec/bitrate and drops resolution limits. Sync requests provider/server transcoding at `hifimule-daemon/src/sync.rs:3365`; there are no local FFmpeg encoding settings in this path.

## Deduced Conclusions

### Deduction 1: Lossless reservations can substantially overestimate lower-resolution material

**Based on:** Fixed bitrate formula, Modern DAP profile and deterministic helper output.

**Reasoning:** 9216 kbps represents 192,000 × 24 × 2 bits/second of raw PCM. The same estimate is assigned to a 16-bit/44.1 kHz track of equal duration, without considering lossless compression. Four minutes reserve 284.84 MB, compared with 42.34 MB of raw stereo 16-bit/44.1 kHz audio. Actual FLAC output is not measured here and depends on the server's effective encoding and audio content.

### Deduction 2: Source resolution is unnecessary for a fixed lossy bitrate estimate

**Reasoning:** Encoded audio bytes follow duration × achieved output bitrate / 8. Input depth/rate do not change this arithmetic. Codec variability, metadata overhead and whether a track is actually copied/reused can still affect accuracy.

### Deduction 3: Basket and prepared-plan totals can diverge

**Based on:** Basket always applies the device bitrate if supplied; daemon direct transfer uses source bytes.

**Conclusion:** A profile-compatible track copied directly can have a different basket reservation from the prepared transfer size, even for lossy targets.

## Hypothesized Paths

### Hypothesis 1: Original depth and sample rate are missing from estimates

**Status:** Confirmed in estimation; attribution to user's files remains open

**Theory:** Source bit depth and sample rate are ignored in cases where they affect encoded size.

**Would confirm:** Estimate formulas lack relevant source fields while actual encoding preserves or changes them.

**Would refute:** Source metadata is used correctly, or fixed target bitrate makes those fields unnecessary.

**Resolution:** Source depth/rate do not enter the basket estimator or sync Song model. This matters for lossless output, but not for the basic fixed-bitrate lossy formula. Refutation pass: daemon direct-copy paths do use exact source bytes, so it would be incorrect to claim all estimates ignore source characteristics or use 9216 universally.

## Missing Evidence

| Gap | Impact | How to Obtain |
| --- | --- | --- |
| Affected source codec, duration, depth, sample rate, target profile, displayed estimate, actual size | Cannot attribute the user's files to one mechanism | One representative source and device/output example |
| Effective server encoder output depth/rate/channels | Cannot establish how resolution is preserved or changed | Provider request trace and output probe for representative encode |

## Source Code Trace

| Element | Detail |
| --- | --- |
| UI trigger | BasketSidebar.ts:185 → syncCapacity.ts:42 plannedSelectionBytes |
| Profile input | rpc.rs:4379 → sync.rs:1706 target_bitrate_kbps, fallback 9216 |
| Prepared transfer | sync.rs:1645 planned_audio_bytes, direct source size or duration/target reservation |
| Actual audio output | sync.rs:3365 provider.download_url; provider/server owns encoding |
| Conditions | Coarse lossless profile/fallback; actual direct/reused files differ from coarse basket reservation |

## Conclusion

**Confidence:** High for estimator mechanism and deterministic calculation; unresolved for the user's particular files

The recent correction accounts for device target bitrate but does not incorporate source bit depth/sample rate into lossless predictions. Modern DAP uses a conservative 24-bit/192 kHz stereo PCM reservation irrespective of source resolution or compression. Direct copies are handled more accurately during preparation than in the coarse basket estimate. No product code changed.

## Recommended Next Steps

**Fix direction:** Use the same direct/reuse/transcode decision in basket and preparation; retain known bytes for copies/reused files. For lossless transcodes, model effective output sample rate, depth and channels plus a clearly identified compression estimate/range. Do not treat FLAC's configured maximum bitrate as an achieved fixed output bitrate. Source metadata alone is insufficient if output policy differs.

**Diagnostic:** Obtain one representative file and target profile; compare basket, prepared-plan and measured output sizes and probe effective output resolution. An optional clarification was sent during investigation.

**Workflow:** `bmad-quick-dev` for implementation once scope is requested; `bmad-code-review` for independent review of the resulting change.

## Reproduction Plan

Execute the actual UI helper for a single 240-second track (`sizeTicks = 240 × 10,000,000`, `childCount = 1`): 192 kbps → 5,998,336 bytes; 9216 kbps → 284,839,936 bytes. Source size changes do not alter either result. Compare 16-bit/44.1 kHz and 24-bit/192 kHz stereo raw PCM arithmetic (42,336,000 and 276,480,000 bytes). Future verification should compare actual provider output without assuming preserved resolution.

## Side Findings

None recorded.

## Follow-up: 2026-10-19

### New Evidence

The user clarified that the issue primarily affects an **MP3 target and Auto-Fill**: the selected tracks take substantially less actual space than estimated, leaving the device far from full. Sources are mixed, including FLAC at different depths and sample rates. The user has no representative sample available. The earlier lossless finding remains valid but does not establish the cause of this symptom.

### Updated Hypotheses

1. **Source FLAC resolution inflates MP3 planning — under investigation.** Production Auto-Fill paths already use target output estimation; direct source-size inflation is not established.
2. **Requested and achieved MP3 bitrate differ — open.** Check provider URL bitrate units, server behavior, per-slot overrides and low-rate codec constraints.
3. **Budget/termination logic underfills independently of encoded size — open.** Check manual reservations, early termination, headroom, duration limits and selection constraints.

### Verification

Generated 12-second stereo FLAC sources locally and encoded using FFmpeg/libmp3lame at requested 320 kbps. At either 16- or 24-bit source depth:

| Input sample rate | Output rate | Output bitrate | Output bytes |
| --- | --- | --- | --- |
| 22,050 Hz | 22,050 Hz | 160,000 bps | 241,937 |
| 44,100 Hz | 44,100 Hz | 320,000 bps | 482,786 |
| 96,000 Hz | 48,000 Hz | 320,000 bps | 481,964 |
| 192,000 Hz | 48,000 Hz | 320,000 bps | 481,964 |

This controlled local experiment shows no substantial MP3 size difference for the tested common/higher source resolutions; the tested low-rate source produces a lower bitrate. It does not establish the user's server encoder settings. Files were generated in a temporary directory and removed after verification.

### Backlog Changes

Trace MP3 Auto-Fill planning through both candidate expansion and actual provider requests. Inspect the Jellyfin forced URL's bitrate units and the condition selecting that route. Independently inspect UI capacity and slot ceilings. Provider and chosen MP3 profile requested as optional context.

### Updated Conclusion

**Confidence:** High for code defects and controlled encoding results; not yet established for the user's exact underfill mechanism.

The original lossless diagnosis does not explain the clarified report. The production Auto-Fill paths already supply the device output profile and estimate FLAC transcodes using duration and the MP3 target bitrate. Higher source bit depth/sample rate did not meaningfully change fixed-bitrate MP3 size in the controlled test. Source resolution therefore should not be used to scale the MP3 byte estimate under these tested conditions.

### Additional Findings

**Confirmed: production planning uses the MP3 target.** Delegated trace found all three production `AutoFillParams` constructions pass `output_profile` (`hifimule-daemon/src/rpc.rs:5756`, `:7647`, `:8202`). Configurable expansion rewrites candidate size to `planned_song_bytes` at `hifimule-daemon/src/auto_fill/fetch.rs:375` and disables `encoding_from_goals` at line 376 to avoid shrinking twice. Default provider and legacy Jellyfin paths also use the shared planner. Thus a general claim that Auto-Fill budgets original FLAC bytes is refuted for the traced production paths.

**Confirmed: Jellyfin fallback sends the wrong bitrate unit.** `hifimule-daemon/src/api.rs:1105` divides `MusicStreamingTranscodingBitrate` by 1000, then inserts that number into `audioBitRate` at lines 1115–1121. A 192,000 bps target becomes `audioBitRate=192`, and 320,000 becomes `320`. The checked-in Jellyfin OpenAPI at `docs/jellyfin-openapi-stable.json:1346` documents an example of 128000 for this parameter. This is a request mismatch by a factor of 1000. Actual low-quality output, server clamping, or rejection depends on the server; no specific server response was observed.

**Confirmed: fallback activation is conditional.** `hifimule-daemon/src/api.rs:1036` first examines PlaybackInfo MediaSources, prefers TranscodingUrl at line 1049, and can return a permitted direct download at line 1078. Otherwise it constructs the forced stream URL at line 1097. Missing/empty MediaSources also reaches this fallback. Normal server-provided TranscodingUrl bypasses the defect. Delegated Git trace dates this defect to commit `980c37b6` (2026-05-13); it was not introduced by the recent estimation correction.

**Confirmed: the UI does not generally subtract managed space twice.** `hifimule-ui/src/state/syncCapacity.ts:19`–`:27` computes free + managed bytes − planned manual bytes once. Default pipeline budgets do not impose duration/headroom limits (`hifimule-ui/src/state/autoFill.ts:213`). Auto-Fill preview displays daemon candidate-size totals (`hifimule-ui/src/components/AutoFillPanel.ts:1310`); it does not recalculate source FLAC sizes. The routine 3% + 64 KiB allowance alone does not explain severe underfill for ordinary-length tracks.

**Conditional causes still requiring configuration evidence:**

- Missing duration reserves one hour per track (`hifimule-daemon/src/sync.rs:1650`); this can overestimate greatly.
- A custom/incomplete MP3 profile without either bitrate field falls back to 9216 kbps (`hifimule-daemon/src/sync.rs:1651`). Stock MP3 profiles contain explicit bitrates.
- Goal-derived bitrate may differ between normalized planning and execution: configurable fetch inserts an effective max-byte ceiling (`hifimule-daemon/src/auto_fill/fetch.rs:254`–`:262`) before deriving bitrate (`:374`); execution derives its override from the original budget (`hifimule-daemon/src/rpc.rs:7118`). If that original budget lacks maxBytes, planning can derive 320 kbps while execution retains the device's 192 kbps target. Relevant only with duration + encoding-from-goals enabled.
- Default selection can stop on the first candidate/unit that does not fit (`hifimule-daemon/src/auto_fill/mod.rs:368`, `hifimule-daemon/src/auto_fill/pipeline.rs:2138`), potentially leaving space even if smaller subsequent candidates would fit.
- Source rates such as 22.05 kHz can constrain achieved MP3 bitrate, as observed locally. Actual source rates and server resampling are unknown.

### Updated Fix Direction

Prioritize the confirmed Jellyfin bitrate unit defect with a mocked fallback regression. Keep planner and encoder bitrate derivation identical for goal-based Auto-Fill. Use output probes or sync logs to determine whether the observed case is an achieved-bitrate mismatch, missing duration, premature selection stop, or explicit budget constraints. No product implementation changes were made in this investigation.

## Follow-up: 2026-10-19 #2

### New Evidence

The user first answered Jellyfin, then corrected the provider to **Navidrome**. Treat Navidrome as the authoritative provider for this case. The Jellyfin unit defect does not explain this run.

### Additional Findings

- **Confirmed locally:** `hifimule-daemon/src/providers/subsonic.rs:737` routes profiled downloads to `stream_url`. At line 1634, the client sends the target container as `format` and `max_bitrate_kbps` directly as `maxBitRate`. For a 192 kbps target this is `format=mp3&maxBitRate=192`; these are the correct Subsonic bitrate units, unlike the unrelated Jellyfin fallback bug.
- **Confirmed upstream capability, version-specific:** Navidrome v0.64.2 `core/stream/legacy_client.go:66`–`:108` reads player settings and applies server overrides or `player.MaxBitRate` before making the stream decision. The selected response bitrate is taken from that decision at lines 118–122. Source: [Navidrome v0.64.2 legacy stream resolver](https://github.com/navidrome/navidrome/blob/v0.64.2/core/stream/legacy_client.go).
- **Confirmed upstream history:** [Navidrome PR #5611](https://github.com/navidrome/navidrome/pull/5611), merged 2026-06-14, documents server-side player MaxBitRate enforcement across stream paths and that the cap can lower a client-requested bitrate. This establishes a mechanism, not the user's installed version or player configuration.
- **Deduced conditional impact:** A HifiMule 320 kbps reservation with effective Navidrome 128 kbps MP3 output consumes roughly 40% of the estimated audio payload; a 192-to-128 mismatch consumes roughly 67%, before overhead. Source FLAC depth/rate does not need to enter this explanation.

### Updated Hypotheses

- **Jellyfin fallback units caused the observed underfill — refuted for this case.** The user corrected the provider to Navidrome; keep the confirmed separate bug in the record.
- **Navidrome caps output below HifiMule's planning bitrate — open.** Confirmed server mechanism and client prediction mismatch; confirm attribution by comparing the selected MP3 target with the HifiMule player's Navidrome max bitrate/transcoding settings and one output's effective bitrate. No settings or output measurements have been observed.
- **Duration fallback/selection stop or goal-derived override mismatch — open.** Continue considering these if requested and actual bitrate match.

### Updated Conclusion

**Confidence:** High for correct client request units and Navidrome's ability to lower the requested bitrate; unconfirmed attribution to the user's run.

Do not claim the Jellyfin bug or mixed high-resolution FLAC caused this Navidrome underfill. A lower effective server bitrate is a concrete remaining explanation because HifiMule currently reserves the requested target rather than an observed/negotiated server output. Diagnose against Navidrome's HifiMule player configuration and output bitrate before choosing a product fix. No product code changed.

## Follow-up: 2026-10-19 #3

### New Evidence

The user supplied an accessible Jellyfin test server and the playlist **80's Rock Running**, ID `06f2ea935fb3164dd657fc2b5108b586`, reporting **583 Mo estimated versus 545 Mo after copying**. This sample is explicitly Jellyfin, separately from the earlier Navidrome report. Authenticated using the supplied test account without recording credentials or access tokens in this report or metadata cache. Server public metadata identifies Jellyfin 12.1.0.

### Confirmed Sample Findings

| Evidence | Value |
| --- | --- |
| Playlist tracks | 91 returned, 91 unique, no missing durations |
| Parent RunTimeTicks and CumulativeRunTimeTicks | 244,699,607,951 ticks = 24,469.9607951 seconds |
| Sum of individual track RunTimeTicks | 229,292,055,291 ticks = 22,929.2055291 seconds |
| Parent excess duration | 1,540.755266 seconds, about 25 min 41 s |
| Sum of per-track integer seconds used by Song/sync | 22,885 seconds |
| Source formats | 59 MP3 and 32 AAC/M4A; no FLAC in this sample |
| Combined reported source bytes | 594,981,058 bytes |

The initial `/Playlists/{id}/Items` response and the **exact production provider query** `/Users/{userId}/Items?ParentId={playlistId}&IncludeItemTypes=Audio,MusicVideo&Fields=MediaSources,ProviderIds&Recursive=true` returned the same 91 IDs and identical summed duration. Both TotalRecordCount values were 91. There is no observed missing page, duplicate ID or endpoint membership mismatch in this sample. The reason the server's parent duration is larger remains unknown; describe it as inconsistent aggregate metadata, not proven cache staleness.

### Deterministic Reproduction

The existing TypeScript `plannedSelectionBytes` helper was executed on the live metadata at **192 kbps**:

| Calculation | Bytes | MiB | Rounded app display |
| --- | --- | --- | --- |
| Parent duration, 91 children, 3% + 64 KiB/track | 610,861,207 | 582.562644 | **583 MB** |
| Summed exact child durations, same allowance | 572,773,737 | 546.239602 | **546 MB** |
| Existing daemon arithmetic: each child's integer seconds, same allowance | 571,680,976 | 545.197464 | **545 MB** |

This reproduces both reported rounded numbers using the two existing estimation paths. The 545 MiB result is a **prepared estimate computed from live metadata**, not an independently measured download of the playlist. The user's post-copy measurement remains user-reported. `BasketSidebar.ts:86`–`:90` formats MB labels by dividing bytes by 1,048,576, so the table uses MiB internally to match its display.

### Confirmed Source Trace

- `hifimule-daemon/src/providers/jellyfin.rs:1245`: playlist conversion derives duration from parent `CumulativeRunTimeTicks`.
- `hifimule-daemon/src/rpc.rs:2595`: browse playlist results serialize this duration.
- `hifimule-ui/src/library.ts:329`: playlist cards set `sizeTicks = durationSeconds × 10,000,000`.
- `hifimule-ui/src/components/MediaCard.ts:250` and `hifimule-ui/src/library.ts:1184`: basket additions prefer existing nonzero `sizeTicks` when counts/sizes arrive.
- `hifimule-daemon/src/rpc.rs:6552`: the playlist counts handler likewise returns the parent cumulative duration, without summing actual tracks.
- `hifimule-ui/src/state/basket.ts:310`: size hydration updates bytes but does not repair duration.
- `hifimule-daemon/src/providers/jellyfin.rs:458` → `hifimule-daemon/src/api.rs:1539`: provider sync retrieves the actual playlist tracks through the query verified above.
- `hifimule-daemon/src/providers/jellyfin.rs:1307` and `hifimule-daemon/src/domain/models.rs:352`: each track duration is converted to integer seconds.
- `hifimule-daemon/src/sync.rs:1650`: prepared output bytes are calculated from each track's duration.

### Updated Hypotheses

- **Inconsistent playlist aggregate duration causes this sample's basket overestimate — confirmed.** The actual helper with server parent metadata reproduces 583 MiB; actual child metadata and existing daemon arithmetic reproduce 545 MiB. Production query membership verified.
- **Source depth/frequency causes this sample — refuted.** No FLAC sources occur in the sample; the duration mismatch explains the two displayed numbers without resolution inputs.
- **Jellyfin fallback bitrate units cause this sample — unsupported and unnecessary.** Remains a separate confirmed code defect, with no observed activation in this sample.
- **Navidrome player cap causes this sample — refuted for this sample's provider.** Still a conditional hypothesis for the earlier Navidrome report only.

### Updated Conclusion and Fix Direction

**Confidence:** High for the inconsistent basket/prepared estimation mechanism; no independent copied-audio byte measurement was performed.

The basket trusts inconsistent playlist aggregate duration, whereas preparation uses individual tracks. This explains the reported 583-versus-545 rounded numbers at 192 kbps. When such a playlist is manually selected alongside Auto-Fill, its inflated reservation also reduces the space offered to Auto-Fill, providing a direct mechanism for underfill. This does not establish that every Auto-Fill-only underfill uses the same mechanism.

Compute playlist capacity from the same expanded track set, per-track duration precision and direct/transcode decisions used in sync. Refresh corrected duration on basket hydration and avoid preserving parent duration over authoritative child sums. Add a meaningful regression using a playlist whose parent cumulative duration disagrees with its children; assert that basket and prepared totals agree. No product implementation changes were made.


## Implementation follow-up: 2026-10-19

The user requested fixes for all confirmed diagnostic defects. The implementation specification is [Consistent device output estimates and Auto-Fill packing](../spec-consistent-sync-size-estimates.md).

The patch addresses these mechanisms:

- Basket estimates expand actual provider tracks and use the same output decisions as preparation, including direct copies, measured resident files, overlapping selections, effective device profiles and whole-second track durations. Cached results are fenced against destination/profile/basket changes and refreshed before sync.
- Source sample rate, depth, channels, original bytes and average bitrate survive provider conversion and planning. Lossy targets use normalized codec defaults and constraints; lossless conversions reserve effective PCM output rather than applying a universal 9216 kbps value. Missing duration is derived from original evidence when available.
- Jellyfin forced fallback requests use bitrate in bits per second and preserve supported output limits. Audio-stream bitrate determines audio compatibility; container average bitrate remains available for duration inference.
- Auto-Fill carries the effective goal-derived bitrate into execution, respects device-preferred formats, skips unfitting atomic selections and continues to later candidates, and paginates priority sources beyond the first 2000 entries with bounded no-progress guards.
- Missing device files are excluded from capacity credit; smaller goal encodings replace oversized retained outputs and clean up the old file after a successful write. Metadata ID reassignment shares measured-byte estimates with preparation and respects server ownership, quality and profile rewrites.
- Force Sync retains resolved duration/source metadata and counts forced ID-reassignment replacements once. Ambiguous cross-server track IDs are rejected before unsafe combined preparation rather than silently dropping a selection.

The server sample is retained as a deterministic 91-track regression: 22,885 whole seconds at 192 kbps, with existing allowances, yields 571,680,976 bytes (545 MiB rounded). No credentials are stored in the regression and no full media download was needed.

Navidrome player/server bitrate caps remain an external configuration uncertainty. The patch does not change server settings or claim that requested output was independently measured. FLAC compression remains content-dependent, so PCM reservations are conservative rather than exact compressed-file predictions. Legacy Subsonic resolution caps are not assumed to be enforced.

Verification and independent-review results are recorded in the implementation specification after completion. Earlier sections preserve the chronological diagnostic evidence and describe the pre-fix behavior.

Independent acceptance and edge reviews also verified exact audio bitrate preservation, enforcement of inferred durations, safe replacement cleanup, and terminal automatic-sync error state. Review findings were repaired and rechecked.

Final validation: 1585 daemon unit tests and 5 provider contracts passed (12 existing unit tests ignored), 180 UI tests passed, the production UI build passed, and patch whitespace is clean.


Parallel-validation follow-up: a user run exposed four pipeline fixture timeouts at the shared two-second first-write wait. A new paused-clock regression reproduced delayed startup and early release. Bounded progress waits and a retained one-writer release signal fixed the test synchronization. Normal parallel validation now passes 1586 daemon unit tests and 5 provider contracts, with 12 existing tests ignored. Production sync code is unchanged by this follow-up.


## Follow-up: 2026-10-20 — Alchemy playlist

### User observation and evidence

The user reports `Alchemy: Appoloosa_instant` (`e546bf88dbe3c17ac3f4ea765adeeaac`, portable server `8d5613157d2547e9b35fd762fe1f253e`) displays 659 MB estimated and 637 MB after copying. The server title is `Alchemy: Appaloosa_instant`. Read-only authenticated metadata was retrieved from the previously supplied Jellyfin test server; no audio download or encode was requested and no credentials were persisted in this report.

The production provider query returns 99 unique tracks, with no missing durations: 35 MP3 and 64 M4A. Audio streams are stereo; 91 report 44.1 kHz and 8 report 32 kHz. No FLAC appears in this sample. Parent cumulative duration is 28,062.2950636 seconds; child durations total 27,731.8518886 seconds; the sum of each track's whole seconds is 27,682.

### Confirmed calculation

At a forced MP3 192 kbps target, current `planned_audio_bytes` arithmetic reproduces the displayed value exactly:

| Component | Bytes | MiB |
| --- | ---: | ---: |
| Whole-track-second audio payload | 664,368,000 | 633.5907 |
| Three-percent allowance | 19,931,040 | 19.0077 |
| 64 KiB × 99 tracks | 6,488,064 | 6.1875 |
| Total reservation | 690,787,104 | 658.7859 → 659 |

Source: `hifimule-daemon/src/sync.rs:1691-1720`. A calculation using the inconsistent parent cumulative duration instead would reserve 667.7513 MiB (668 rounded), refuting the earlier aggregate-duration defect as the explanation for the reported 659 in this sample. Allowing direct copies of the 13 MP3 tracks at/below 192 kbps would produce a different total (641.9072 MiB), so the exact numeric match points to the all-encoded 192 kbps policy; the active device profile was not independently inspected in this follow-up.

### Conclusion and limits

**Confirmed:** The 659 display is reproduced by the current encoded-output capacity reservation, including approximately 25.2 MiB of allowances. Full-precision source duration corresponds to 634.7317 MiB of 192 kbps audio payload, before stream/container/tag overhead. The user's 637 MiB copied result is compatible with an encoded output below the conservative reservation.

**Deduced:** The remaining roughly 22 MiB difference is consistent with unused reservation allowance, not evidence of a renewed playlist-duration or source-resolution defect. Post-copy output bytes remain user-reported; actual MP3 stream bytes, embedded metadata and the active device profile were not independently measured here. No precise decomposition of the 637 result is claimed.

**Fix direction:** The UI currently presents a conservative capacity reservation as the estimated output size. A product change could show the expected audio/output estimate and safety reserve separately while retaining safe capacity budgeting. Simply deleting the safety allowance would change Auto-Fill's capacity policy and is not justified by metadata alone.


### Source verification

A delegated read-only trace confirmed that the stock `rockbox-mp3-192` profile explicitly sets 192,000 bps and an empty direct-play list (`hifimule-daemon/assets/device-profiles.json:39-49`), making the all-encoded reproduction a match to an existing stock policy. The active destination profile remains unobserved.

New provider transfers record counted HTTP/staging bytes as manifest `size_bytes` (`hifimule-daemon/src/sync.rs:3930`, `:4425`, `:5073-5104`). The basket estimator prefers valid resident manifest sizes (`hifimule-daemon/src/sync.rs:1758-1776`), and completed-sync time participates in cache invalidation (`hifimule-ui/src/state/basketEstimate.ts:22-28`). This explains how a post-copy total can replace the pre-copy reservation without changing playlist membership or target settings. No additional product defect was confirmed for this sample; no capacity allowance was removed.

**Confidence:** High for reproducing the 659 estimate and identifying its safety allowances; medium for attributing the entire user-reported 659-to-637 transition without the actual destination manifest/output files. Exact copied MP3 payload and metadata overhead remain unmeasured.


### Implemented policy: 2026-10-20

The user approved separating expected output from safety reserve and replacing the percentage allowance with 1%. The existing 64 KiB per converted track remains. Planning shares one breakdown across basket estimates and prepared sync summaries; capacity admission and Auto-Fill consume the full reserved total. Direct copies and valid measured resident outputs have zero additional reserve. Legacy daemon totals remain authoritative without an invented breakdown.

For the supplied Alchemy sample, the expected payload remains 664,368,000 bytes (633.5907 MiB). The new reserve is 13,131,744 bytes (12.5234 MiB), and the capacity reservation is 677,499,744 bytes (646.1141 MiB). The UI rounds these independently to 634 MB expected and 13 MB reserve; capacity uses the unrounded total. These are metadata-based calculations; the actual copied 637 MiB remains user-reported.

Implementation and verification are recorded in [the expected-size and reserve specification](../spec-expected-size-and-one-percent-reserve.md). The earlier 3% calculations above preserve the evidence for the original 659 MiB display.
