---
title: 'Start Radio without source configuration'
type: 'feature'
created: '2026-10-01'
status: 'done'
baseline_commit: 'c865aa7ff5da8b5daa4f8086ecf6ae3da33cdb3b'
context:
  - '{project-root}/_bmad-output/planning-artifacts/project-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The Radio button fails with a setup error when no Playback sources have been configured. Users should be able to start listening immediately after connecting a music library.

**Approach:** Treat an empty Playback source list as automatic whole-library mode. At each new Radio start, use the current music library, otherwise the first configured music library. Draw from the provider's global song collection, as autosync does, through the existing shared selection engine and bounded Radio replenishment.

## Boundaries & Constraints

**Always:** Resolve the default in the daemon so UI and native menu starts behave alike. Use portable server identities for playback. Exclude audiobook and podcast libraries. Preserve saved ordering, seed, track budget, explicit sources, output selection requirements, cancellation fences, session exclusions and existing queues on failed starts. Keep provider work bounded. Freeze the resolved library in the admitted session so subsequent browsing changes cannot reroute it.

**Ask First:** Expanding the default to multiple libraries at once or allowing book/podcast libraries as music sources.

**Never:** Rewrite physical-device autosync settings, persist an implicitly chosen library into the user's Playback source configuration, replace invalid or unavailable explicit sources with a whole-library fallback, bypass damaged/future configuration errors, or enqueue the entire collection.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|---------------------------|----------------|
| Current music library | Missing configuration or empty sources; selected music server | Start Radio from that server's global library | Existing output/preparation errors apply |
| Current nonmusic library | Empty sources; selected audiobook/podcast server | Use first music server in existing configured-server order | Do not alter browse selection |
| No current selection | Empty sources; music servers exist | Use first configured music server | Same start checks as other sources |
| No music library | Empty sources; no music server | Explain that a music server must be connected | Leave existing session intact |
| Explicit sources | Nonempty saved sources | Use those sources and existing selection settings | Preserve current failure semantics |
| Empty or unavailable chosen library | Default resolves but yields no music or fetch fails | Report empty/source error | Do not silently try unrelated libraries |
| Browse switch during Radio | Default Radio admitted, current library changes | Continue original resolved library | Preserve session restoration identity |
| Large global collection | More tracks than one bounded window | Start from bounded candidates and advance refill cursors | Distinguish fetch failure from exhaustion |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/playback/selection.rs` — typed Playback sources, validation, bounded initial fetch and persisted Radio scan cursors.
- `hifimule-daemon/src/rpc/playback_selection.rs` — config loading, provider routing, fenced Radio admission and replenishment worker.
- `hifimule-daemon/src/server_manager.rs` and `hifimule-daemon/src/db.rs` — selected server, portable identities, library roles and deterministic configured-server ordering.
- `hifimule-daemon/src/auto_fill/fetch.rs` — autosync global collection semantics: `list_all_songs_page(None, offset, limit)`.
- `hifimule-ui/src/components/PlaybackSelectionSettings.ts` — empty-source start currently disabled; saved sources edited here.
- `hifimule-ui/src/components/PlaybackControls.ts` and `hifimule-ui/src/rpc.ts` — primary Radio button and typed RPC contract.
- `hifimule-i18n/catalog.json` — shared English, French, Spanish and German UI/native messages.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/playback/selection.rs` — add a typed library source with appropriate no-reference validation and bounded provider fetching for startup and successive Radio windows; update exhaustive source matches without weakening explicit-source validation.
- [x] `hifimule-daemon/src/rpc/playback_selection.rs` — resolve empty configurations to the selected music server or first music server; keep the resolution ephemeral and capture it in session settings. Accommodate library sources at RPC boundaries and return a distinct actionable no-music-server error.
- [x] `hifimule-ui/src/rpc.ts` and `hifimule-ui/src/components/PlaybackSelectionSettings.ts` — support the source contract and allow saved empty configurations to start automatic Radio; retain dirty/save guards and valid explicit-source controls.
- [x] `hifimule-ui/src/components/PlaybackControls.ts` and `hifimule-i18n/catalog.json` — explain automatic mode in settings and show the music-server requirement when defaults cannot resolve, across supported languages and native consumers.
- [x] `hifimule-daemon/src/playback/selection.rs`, `hifimule-daemon/src/rpc/playback_selection.rs` and `hifimule-ui/tests/radioDefaults.test.mjs` — cover matrix boundaries, global provider calls, paging, portable identities, explicit-source preservation and empty-source UI behavior using existing test patterns.

**Acceptance Criteria:**
- Given a connected music library and no saved source configuration, when Radio starts from the main button or desktop menu, then music plays without opening settings.
- Given a default Radio session, when it replenishes or restores after browsing another library, then its resolved global source and existing Radio rules remain intact.
- Given configured explicit artist, playlist or genre sources, when Radio starts, then their existing behavior remains intact.
- Given a failed default Radio start, when the error is displayed, then the existing listening session remains unchanged.

## Spec Change Log

## Design Notes

The provider's global collection is requested without a library filter, matching autosync. This is a source scope, not a request to download or enqueue every track. Initial candidate fetching and later cursor windows must retain Playback's existing limits. An empty persisted source list remains dynamic for future starts; the admitted session stores a concrete library source for continuity.

## Verification

**Commands:**
- `rtk cargo test -p hifimule-daemon playback::selection` — source validation and bounded fetching tests pass.
- `rtk cargo test -p hifimule-daemon rpc::playback_selection` — default resolution and start regressions pass.
- `rtk cargo test -p hifimule-daemon playback::radio` — replenishment/session behavior remains valid.
- `rtk proxy node --test hifimule-ui/tests/radioDefaults.test.mjs` — default UI behavior passes.
- `rtk npm run build --prefix hifimule-ui` — TypeScript and production UI build pass.

**Implementation verification (2026-10-01):**
- Daemon tests ran through `rtk npm run build:daemon -- test -p hifimule-daemon <filter>` because the repository build script requires controlled FFmpeg verification before Cargo. Selection: 21 passed; RPC default resolution: 4 passed; Radio: 37 passed; failed-start session preservation regression: 1 passed.
- `rtk proxy node --test hifimule-ui/tests/radioDefaults.test.mjs`: 3 passed.
- `rtk npm run build --prefix hifimule-ui`: passed (existing bundling warnings).
- `rtk git diff --check`: passed.
- Automatic defaults remain ephemeral; the concrete global source is serialized only with session settings. Added shared translations and native/row error mappings for the music-server requirement.

## Review Results

Blind hunter, edge-case hunter and acceptance auditor completed independent reviews with no actionable findings.

## Suggested Review Order

**Default selection**

- Resolve one music library per start while preserving explicit sources and saved defaults.
  [playback_selection.rs:409](../../hifimule-daemon/src/rpc/playback_selection.rs#L409)

**Bounded global source**

- Use autosync’s global song scope while retaining bounded Radio paging.
  [selection.rs:849](../../hifimule-daemon/src/playback/selection.rs#L849)

**Source validation**

- Allow reference-free library sources without weakening explicit-source validation.
  [selection.rs:87](../../hifimule-daemon/src/playback/selection.rs#L87)

**Settings behavior**

- Enable automatic starts while preserving dirty, saving and busy guards.
  [PlaybackSelectionSettings.ts:108](../../hifimule-ui/src/components/PlaybackSelectionSettings.ts#L108)

**Regression coverage**

- Verify selected music, fallback ordering, portable identity and frozen session scope.
  [playback_selection.rs:63](../../hifimule-daemon/src/rpc/playback_selection.rs#L63)

- Verify empty-source starts and actionable failures through the existing settings component.
  [radioDefaults.test.mjs:32](../../hifimule-ui/tests/radioDefaults.test.mjs#L32)
