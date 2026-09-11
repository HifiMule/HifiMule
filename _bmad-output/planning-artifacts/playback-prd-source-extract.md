# Playback PRD Source Extraction

Date: 2026-09-11. Planning extraction only; no production implementation or new approval implied.

## Sources and precedence

- `prd.md`: existing product baseline; highest existing functional requirement identifier is **FR54**. Preserve existing IDs and begin additive playback requirements at FR55.
- `architecture.md`, Playback Extension (from line 940): explicitly approved playback decisions and later validation refinements supersede incompatible legacy statements and unresolved early brainstorming proposals.
- `../brainstorming/brainstorming-session-2026-09-11-090745.md`: accepted product intent, ideas #1–46. Questions and assistant proposals are not approvals; later architecture resolves several questions.

## Approved product requirements to preserve

1. Everyday desktop listening and curation previews on Windows, macOS and Linux; daemon-owned playback continues with UI closed and responds to native media controls. Full Quit saves and stops; restore queue, position and logical-session exclusions paused. Single interactive-user daemon and safe coordination with active sync are required.
2. Ordered album playback preserves disc/track order, gapless continuity and recorded silence; remove only known encoder padding. Shared output is default; OS owns Do Not Disturb. Output selection is supported; output loss pauses without rerouting and reconnection does not auto-resume.
3. Full-track Preview preserves one main queue, track, position and previous playing/paused state. Another preview replaces the audition without nesting. Natural completion restores saved state; explicit Stop restores main paused; no main session means idle after completion. Failed return retains recoverable main state.
4. Continuous editable local Radio reuses the pure selection engine with Playback-owned settings and server sources. Play something starts a fresh Radio from the first eligible engine selection; Resume continues an existing session. Both are available from the app menu without opening the UI.
5. Radio stays with the current artist while eligible unheard tracks remain, then follows meaningful supported artist relationships with honest explanations/provenance. Initially use configured-server metadata only. Missing relationships trigger an explicitly labeled fresh center under original selection settings. Exhausted unheard candidates start another cycle retaining exclusions; no eligible candidates means an explained waiting state.
6. Bound upcoming lookahead and replenish incrementally without reordering manual edits; separately bound candidate work, compressed prefetch and PCM. Store long history durably and page it for display. Skipped and manually removed automatic suggestions remain excluded for the logical session, including restart; a new Radio resets exclusions. No cross-session local taste model.
7. Cross-server playback routes by portable source-server plus provider track identity independently of browse context. Conservatively deduplicate same recordings while retaining source alternatives; uncertain matches and distinct performances remain separate. Queue occurrences retain separate identities.
8. Choose highest sustainable desktop quality independently of portable transcoding profiles. Brief startup buffering and automatic buffer-informed adaptation are accepted. Initial quality changes occur at track boundaries; mid-track continuity requires provider validation. Surface buffering/retry when no representation is sustainable. Radio may bypass technical unavailability; albums pause/retry without omitting tracks. Technical failure is not user rejection.
9. Match loudness with track gain for Radio, consistent album gain for albums and peak protection. Missing usable metadata leaves gain unchanged. No dynamic-range compression or implicit background loudness analysis is required.
10. Playback and sync normally coexist. Back off sync only on actual playback risk; validate album continuity and sync throughput under a large real sync.
11. Playback is the first always-present virtual destination, with independent configuration and manual queue/automatic selection. Select it when no physical device remains. Physical arrival selects its context without stopping music; show detection/open failures and preserve explicit blank-device setup. The destination never performs physical file sync.
12. Persistent translucent floating controls under the browser remain across device views and while idle, offering Play something. Keep final list rows accessible. Explicit Preview is distinct from Play; precise preview entry placement and the final control layout remain design work.
13. Explicit immutable save snapshots contain accepted played history, current occurrence once unless rejected, and upcoming entries, omitting skipped/disliked entries while preserving deliberate repeated occurrences. Saving does not create a live link. Split server saves into ordered per-server playlists with independent outcomes and safe retry/reconciliation. Connected-device export offers distinct Add and Replace basket actions.
14. Separate now-playing from completed-listen reporting; completed previews count where supported and avoid counting skipped/interrupted tracks where possible. Durable reporting tracking is required, but undo and remote exactly-once effects are not promised. Like/Dislike is exposed only for genuine provider equivalents; un-favorite is not dislike and unsupported actions must not create local taste storage.
15. Daemon is authoritative; reconnect restores a versioned snapshot, stale queue edits conflict, duplicate commands have bounded suppression, and asynchronous results cannot mutate replacement sessions. Credentials/authenticated stream URLs remain daemon-side behind provider adapters.

## Existing PRD conflicts and required reconciliation

- **FR33** locks basket/add actions with no physical selection: retain this for physical sync, explicitly allow Playback/library listening with no connected device.
- **FR20**, headless-sidecar MVP and user-session-daemon post-MVP split: playback needs independent signed-in-user daemon lifetime now within the playback extension; closing UI must not kill audio. Retain sync safety.
- **FR49–FR53** describe physical-device manifests, per-device/server config and capacity budgets: preserve those sync semantics, add separate local Playback configuration, session state and track-count lookahead; do not put Playback state in a physical manifest.
- **FR31/FR32** concern device transcoding: Playback quality must be independent, capability-aware, and must not inherit unsafe direct-download fallback as a continuity promise.
- **FR17–FR19** concern Rockbox scrobbles/Jellyfin history: add capability-specific live listening/preview reporting without replacing device-log behavior or asserting universal server semantics.
- **FR37/FR38/FR40** server/device playlist operations are not an evolving local Radio queue. Add immutable multi-server snapshots and explicit device Add/Replace exports with their own requirements.
- **Resource language** (<10 MB, zero-footprint, minimal resource usage) must remain an idle target, not an active playback guarantee. Active budgets require measurements; preserve existing sync performance requirements with conditional backoff qualification.
- **Scope/journeys/project classification** remain sync-only and greenfield despite a mature foundation; extend with device-independent listening, album continuity, preview curation, Radio and restore journeys and brownfield playback context.
- **Privacy** remains compatible with configured-server-only metadata; do not promote preliminary external MusicBrainz research into an approved service dependency.
- Legacy PRD MVP/post-MVP transcoding duplication is pre-existing; avoid unrelated scope rewriting while making playback scope unambiguous.

## Validation limits and planning gates

Approved architecture selects native FFmpeg + CPAL with a controlled FFmpeg runtime. Tested ARM64 fixtures/native output are feasibility evidence, not physical gapless, streaming adaptation, x64 parity or release certification. Close lifecycle/authenticated-local-access/safe-shutdown contracts; versioned command/state schemas, generation fencing, migrations/config validation; and shipping runtime/version gates in the first applicable story. Exact reporting thresholds, stream seek behavior, resource budgets and detailed UX require specification and validation, not invented commitments.

Stage order: lifecycle → basic streaming/native player → albums/previews → destination/queue UI → Radio → reporting/feedback/exports → reliability/release. Architecture is ready for staged story planning, not unrestricted implementation.
