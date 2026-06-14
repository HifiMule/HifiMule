---
baseline_commit: db9f8eab2465f1e03e4084cc75261e2a4775f952
---

# Story 12.7: Auto-Fill RPC/State Contract & i18n

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want the daemon contract and UI state fully wired and verified for per-server auto-fill pipelines — including the live preview endpoint that currently has no consumer — with i18n completeness locked,
so that auto-fill config persists, previews, and is exposed consistently across the whole feature, closing out Epic 12.

## Scope decision (read first — this is the Epic 12 closeout, NOT a from-scratch build)

**Story 12.6 was deliberately shipped as a vertical slice and absorbed the bulk of 12.7's original scope.** The daemon RPC/state contract, the configuration UI, the per-server slot cards, the multi-slot sync payload, and the i18n catalog were all built and reviewed in 12.6 ([12-6-autofill-configuration-ui-and-multi-server-slots.md](12-6-autofill-configuration-ui-and-multi-server-slots.md), Status: done; 519 daemon tests, frontend tsc+build green). The 12.6 scope note (line 19) and Open Question #1 explicitly recorded that **12.7 shrinks to "contract polish + full i18n completeness + any remaining state wiring."** This story is exactly that — and the residual work has been verified by reading the current code at baseline `db9f8ea`:

**Already built in 12.6 (DO NOT rebuild — verify only):**
- `autoFill.setPipeline { serverId, pipeline }` RPC — dispatch [hifimule-daemon/src/rpc.rs:234], handler `handle_auto_fill_set_pipeline` [rpc.rs:6094-6143]. Validates `serverId`/`pipeline`, atomic persist, returns `{ status, serverId }`, never touches `auto_sync_on_connect`.
- `get_daemon_state.autoFill` emits `{ enabled, maxBytes, pipelines: { <serverId>: <pipeline> } }` [rpc.rs:1859-1867].
- `basket.autoFill`+`serverId`(+inline `pipeline`) per-server provider routing [rpc.rs:5871-5965] — **built and tested, but has ZERO UI callers** (see gap below).
- `autoSyncOnConnect` decoupled — configured via standalone `device_set_auto_sync_on_connect` [rpc.rs:5804-5850]; `autoFill.setPipeline` does not read/write it.
- Pipeline-builder UI ([hifimule-ui/src/components/AutoFillPanel.ts], 470 lines), per-server slot cards + hydration ([BasketSidebar.ts:345-460]), multi-slot sync payload, and 52 `basket.autofill.*` i18n keys in **all four** languages (en/fr/es/de — verified full parity).

**The genuine residual work for 12.7 (this story):**
1. **Live preview affordance** — wire the `basket.autoFill`+serverId endpoint (built in 12.6, no consumer) into the UI as an **explicit, debounced** preview so the user sees what the *current* pipeline would actually fill (real provider-routed track count + size), not just the local byte-ceiling estimate. The epic lists `basket.autoFill (+serverId)` under 12.7 [epics.md:3076]; this is the one contract endpoint with no UI consumer. (12.6 explicitly deferred this: AC12 "may back an explicit, debounced preview affordance but must not fire on every render/toggle.")
2. **i18n completeness audit & lock** — confirm 52-key × 4-language parity (already holds), confirm no raw/hard-coded auto-fill strings remain, and add keys for the new preview UI in all four languages.
3. **State-contract verification & regression locks** — assert (with tests where missing) the contract invariants the epic names: `get_daemon_state` exposes per-server configs; `autoFill.setPipeline` validation/atomicity/other-server-untouched; `autoSyncOnConnect` server-independence. Most of this is already covered (see Dev Notes test inventory) — 12.7 closes any gap and adds the preview path's tests.

**Out of scope (Epic 13, do not touch):** the reserved Memory fields (`stableCorePct`, `repeatTolerance`, `tiers`) remain config placeholders persisted verbatim; the `autofill_history` DB table stays unconsumed; no provider-trait additions; no new selection/budget logic. Do **not** modify the pure engine (`auto_fill/pipeline.rs`, `fetch.rs`).

## Context — what is already built (read before writing code)

You are completing wiring on a fully-built feature. The selection engine, manifest schema, RPC surface, and UI panel all exist and are tested. Reuse everything; add the thin preview consumer + audit/lock the rest.

**Daemon — the preview endpoint you must consume (do NOT modify it):**
- `basket.autoFill` [rpc.rs:5871-5965]. With a non-blank `serverId`, it resolves the provider via `get_provider_by_server_id_for`, picks the pipeline by precedence **inline `pipeline` → persisted `pipelines[serverId]` → `default_legacy`**, resolves `max_fill_bytes` from `maxBytes` param ?? `pipeline.budget.maxBytes` ?? device free bytes, then expands through the **shared sync-time seam** `expand_auto_fill_slot` and returns the **serialized ranked item array** (`Ok(items)` → `serde_json::to_value(items)`). Unknown/unroutable `serverId` → `ERR_CONNECTION_FAILED` (never a panic). Absent `serverId` → unchanged legacy Jellyfin `run_auto_fill` path. Each returned item carries at least `id`, `name`, `sizeBytes` (the same item shape the sync-time expansion produces; confirm exact fields by reading the serialized type before mapping). [Source: rpc.rs:5860-5965]
- **Inline-pipeline parse:** a malformed `pipeline` returns `ERR_INVALID_PARAMS` [rpc.rs:5904-5912] — the preview must surface that as a user-visible error, not a silent failure.

**Daemon — state/config contract (verify, don't change):**
- `handle_get_daemon_state` autoFill block [rpc.rs:1859-1867]: `{ "enabled": d.auto_fill.enabled_for(selected), "maxBytes": d.auto_fill.max_bytes_for(selected), "pipelines": &d.auto_fill.pipelines }`. `None` device → `auto_fill = None`. Legacy device → `pipelines: {}`.
- `handle_auto_fill_set_pipeline` [rpc.rs:6094-6143]: trims `serverId` (blank → `ERR_INVALID_PARAMS`), parses `pipeline` (malformed → `ERR_INVALID_PARAMS`), persists via `update_manifest(|m| m.auto_fill.set_pipeline(...))` (→ `ERR_STORAGE_ERROR` on failure), returns `{ status: "success", serverId }`. `AutoFillConfig::set_pipeline` clears the parked `legacy` block and leaves other servers untouched [device/mod.rs].
- `device_set_auto_sync_on_connect` [rpc.rs:5804-5850] is the sole writer of `auto_sync_on_connect` (besides legacy `sync.setAutoFill` [rpc.rs:6164-6188]). Per-server pipelines never touch it.

**Frontend — what you extend (all in `hifimule-ui/`):**
- `AutoFillPanel.ts` (the modal pipeline builder) — `open()` [AutoFillPanel.ts:84], `renderBody()` [102-132], footer buttons (Cancel/Save) [125-129], `bindEvents()` [265-363], `captureInputs()` [372-378], and `handleSave()` [405-443] which reads inputs into `this.pipeline`, does the GB→bytes/hours→secs budget conversions, then `serializePipeline(this.pipeline)`. **The preview must reuse the exact same "read inputs → materialize → serialize" path** so it previews the *current unsaved* config — extract that body from `handleSave()` into a private `buildPipeline(): AutoFillPipeline` and call it from both Save and Preview (prevents a Save/Preview divergence bug). The panel already holds `opts.serverId` and the in-memory `pipeline`.
- `BasketSidebar.ts` — `openAutoFillPanel()` [BasketSidebar.ts:429-460] constructs the panel with `serverId`, `serverLabel`, `pipeline`, `modes`, `playlists`, `onSave`. The slot card readout is derived locally in `slotSizeBytes()` [388-400] and `upsertAutoFillSlot()` [402-414] — **leave the local readout as the default; the preview is the explicit on-demand affordance, not a replacement.** `getManualItemIdsForServer` exists for per-server exclude sets (added in 12.6) — use it to pass accurate `excludeItemIds` to the preview.
- `rpc.ts` — `rpcCall(method, params)` [rpc.ts:93]; the codebase convention is a typed helper per RPC (e.g. `fetchBrowsePlaylists` [rpc.ts:262], `fetchBrowseModes` [rpc.ts:217]). Add a typed `previewAutoFill(...)` helper mirroring that pattern. `BrowsePlaylist`/`BrowseTrack` interfaces live here [rpc.ts:183-206] — model the preview item type the same way (or reuse an existing basket-item type if one matches the returned shape — check before inventing).
- `i18n.ts::t(key, replacements)` with `{placeholder}` substitution; catalog `hifimule-i18n/catalog.json` languages **en, fr, es, de**; auto-fill keys under `basket.autofill.*` (52 keys, full parity). `t()` falls back to the raw key if missing.
- Existing UI primitives to reuse for the preview: `<sl-button>` with `loading` attribute for the in-flight state, `<sl-spinner>`, the `toast` CustomEvent for errors (`window.dispatchEvent(new CustomEvent('toast', { detail: { type: 'error', message } }))` — pattern at [BasketSidebar.ts:425]).

## Acceptance Criteria

### Live preview affordance (the unbuilt contract consumer)

1. **A "Preview" affordance exists in the Auto-Fill configuration panel.** The `AutoFillPanel` (opened per selected server) gains an explicit **Preview** control (button) that, when invoked, computes the current in-memory pipeline (via the same input-capture/serialize path as Save) and calls `basket.autoFill { serverId, pipeline: <current serialized pipeline>, excludeItemIds: <selected server's manual item ids>, maxBytes? }`. The result is summarized in the panel as **"~{count} tracks · ~{size}"** (size formatted with the project's existing byte-formatting helper; reuse it, do not reinvent). [Source: epics.md:3076 (basket.autoFill+serverId under 12.7); architecture.md:826; rpc.rs:5871-5965]

2. **Preview never fires on every render/edit — it is explicit and debounced.** No `basket.autoFill` call is made on panel open, on input edit, on advanced-toggle, or on slot reconfigure. Preview fires only on the explicit user action, and rapid repeated invocations are debounced (≥300 ms) so the daemon isn't hammered. While a preview request is in flight the control shows a loading state and is not re-fired. This preserves 12.6 AC12's invariant that "no API call is made when auto-fill is toggled or reconfigured." [Source: 12-6 AC12; ux-design-specification.md:101]

3. **Preview reflects the current unsaved pipeline.** The preview uses the in-memory edits (including Advanced-stage values) — not the last persisted pipeline — so the user sees the effect of changes before saving. The materialization path is shared with Save (`buildPipeline()`); a Default-state pipeline previews identically to the legacy fill. [Source: AutoFillPanel.ts:405-443; pipeline.rs:197-216]

4. **Preview errors are surfaced, never silent.** An unknown/unroutable `serverId` (`ERR_CONNECTION_FAILED`), a malformed pipeline (`ERR_INVALID_PARAMS`), or any RPC failure shows a user-visible message (inline in the panel and/or a toast) and clears the loading state. A successful preview returning zero items shows an explicit "no tracks match" message rather than a blank or "~0 tracks" that reads like an error. [Source: rpc.rs:5904-5912, 5938-5945]

5. **Capability/selection guards.** The preview control is only active when a server is selected (`currentServerId != null`, consistent with the panel itself). It routes strictly through `serverId` (portable id) — never the legacy no-serverId Jellyfin path — so it previews correctly for Subsonic/Navidrome servers. [Source: architecture.md#Enforcement:921; rpc.rs:5888-5896]

### i18n completeness (audit & lock)

6. **All new preview strings are translated in all four languages.** Every new UI string introduced for the preview (button label, "~N tracks · ~size" template, loading text, empty-result text, error text) is a `t()` key under `basket.autofill.*` added to **en, fr, es, de**. No new string is hard-coded or left as a raw fallback key. [Source: i18n.ts; catalog.json; epics.md:3077]

7. **Existing auto-fill i18n parity is verified and preserved.** The audit confirms `basket.autofill.*` has identical key sets across all four languages (currently 52 keys each). No existing key is renamed or removed (extend only). The verification is recorded in the Dev Agent Record (e.g., a key-count assertion or a documented check). [Source: catalog.json; 12-6 i18n delivery]

8. **No raw/hard-coded auto-fill strings remain anywhere in the feature.** A sweep of `AutoFillPanel.ts`, `BasketSidebar.ts` (auto-fill regions), and `state/autoFill.ts` confirms every user-facing string flows through `t()`. (Baseline audit found none outside `t()`; this AC locks it including the new preview code.) [Source: AutoFillPanel.ts; BasketSidebar.ts]

### State-contract verification & regression locks

9. **`get_daemon_state` exposes per-server configs (verified).** A test asserts `get_daemon_state.autoFill` carries `pipelines` covering every configured server alongside the retained `enabled`/`maxBytes`; a legacy device yields `pipelines: {}`; no device yields `autoFill: null`. (Cover any gap left by existing tests — see Dev Notes inventory; do not duplicate existing coverage.) [Source: rpc.rs:1859-1867; architecture.md:794]

10. **`autoFill.setPipeline` invariants hold (verified).** Tests assert: blank `serverId` → `ERR_INVALID_PARAMS`; malformed `pipeline` → `ERR_INVALID_PARAMS`; a successful write persists into `pipelines[serverId]`, clears any parked legacy block, and leaves other servers' entries byte-for-byte unchanged; and the RPC neither reads nor writes `auto_sync_on_connect`. [Source: rpc.rs:6094-6143; device/mod.rs set_pipeline; architecture.md#Enforcement:920-922]

11. **`basket.autoFill`+serverId routing is tested for the preview path.** Tests cover: a configured per-server inline pipeline routes to the correct provider and returns ranked items; an unknown `serverId` returns `ERR_CONNECTION_FAILED` (no panic); a malformed inline `pipeline` returns `ERR_INVALID_PARAMS`; absent `serverId` preserves the legacy path. (Extend existing routing tests if they don't already cover the inline-pipeline + error cases.) [Source: rpc.rs:5871-5965]

12. **`autoSyncOnConnect` server-independence preserved.** A test/verification confirms `auto_sync_on_connect` is settable only via `device_set_auto_sync_on_connect` (and legacy `sync.setAutoFill`), is unaffected by `autoFill.setPipeline` writes, and is not duplicated per server. [Source: rpc.rs:5804-5850, 6094-6143; architecture.md:825]

### Quality gates

13. **Daemon build & tests green.** `rtk cargo test -p hifimule-daemon` passes with no regressions (baseline 519 tests from Story 12.6) plus any new preview-path/contract tests. `rtk cargo clippy -p hifimule-daemon --all-targets` adds no new warnings.

14. **Frontend builds & typechecks.** `cd hifimule-ui && npx tsc --noEmit` passes (project's typecheck; no vitest harness exists). `pnpm build` (`tsc && vite build`) succeeds. The preview's request JSON matches the daemon serde shape exactly (`kind` camelCase, `ref` not `refId`, `playCount`/`dateCreated` ordering keys, omit-when-unset) — it reuses `serializePipeline`, so this follows for free, but verify after the `buildPipeline()` refactor.

15. **Zero regression for the shipped 12.6 behavior.** The configuration panel still saves via `autoFill.setPipeline`; slot cards still derive their readout locally with no RPC on toggle/reconfigure; legacy single-server devices still round-trip with empty `pipelines` and no migration. Adding the preview does not change any of these. [Source: 12-6 AC12, AC19]

## Tasks / Subtasks

- [x] **Daemon: verify/lock the state-contract invariants (no production code change expected)** (AC: 9, 10, 11, 12)
  - [x] Read the existing test inventory (Dev Notes) and identify any uncovered assertion among AC9–AC12. Add only the missing tests — do not duplicate.
  - [x] Ensure `basket.autoFill`+serverId tests cover: inline pipeline routes to correct provider, unknown serverId → `ERR_CONNECTION_FAILED`, malformed inline pipeline → `ERR_INVALID_PARAMS`, absent serverId → legacy path. (`test_auto_fill_needs_configurable_routing` [rpc.rs:7814] and `test_auto_fill_budget_headroom_forces_routing` [rpc.rs:7864] exist — extend if the error/inline cases are missing.)
  - [x] If all invariants are already covered, record that finding in the Dev Agent Record rather than adding redundant tests.

- [x] **Frontend: extract `buildPipeline()` from `handleSave()`** (AC: 3, 14)
  - [x] Refactor `AutoFillPanel.handleSave()` [AutoFillPanel.ts:405-443] so the "captureInputs → write budget/genre/memory into `this.pipeline` → `serializePipeline`" body lives in a reusable `private buildPipeline(): AutoFillPipeline`. `handleSave()` calls it then persists; the preview calls it without persisting. Preserve the 12.5 lesson (emit null/omit, not 0, for cleared duration/headroom).

- [x] **Frontend: `previewAutoFill` rpc helper** (AC: 1, 4, 5)
  - [x] In `rpc.ts`, add a typed `previewAutoFill({ serverId, pipeline, excludeItemIds?, maxBytes? })` calling `rpcCall('basket.autoFill', …)`, returning a typed array of preview items (reuse an existing basket-item type if the serialized shape matches — inspect the daemon-returned item fields first; otherwise define a minimal `AutoFillPreviewItem { id; name; sizeBytes }`).
  - [x] Route strictly via `serverId` (portable id); never the no-serverId path.

- [x] **Frontend: Preview affordance in the panel** (AC: 1, 2, 3, 4, 5, 6, 15)
  - [x] Add a Preview button to the panel footer (next to Cancel/Save) wired to: `captureInputs()` → `buildPipeline()` → `previewAutoFill({ serverId: opts.serverId, pipeline, excludeItemIds: <selected server's manual ids via getManualItemIdsForServer>, maxBytes? })`.
  - [x] Debounce (≥300 ms) and guard against concurrent in-flight requests; show a loading state on the button; never fire on open/edit/toggle.
  - [x] Render the result summary "~{count} tracks · ~{size}" using the project's byte-formatter and `t()` keys; show explicit empty-result and error messages (inline and/or toast). Clear loading on all outcomes.
  - [x] Pass the manual exclude ids from `BasketSidebar` into the panel (extend `AutoFillPanelOptions` with a getter/array, or pass `getManualItemIdsForServer(serverId)` result at `openAutoFillPanel()` time [BasketSidebar.ts:429-460]).

- [x] **i18n: preview keys + completeness audit** (AC: 6, 7, 8)
  - [x] Add new `basket.autofill.*` preview keys (button, result template with `{count}`/`{size}` placeholders, loading, empty, error) to **en/fr/es/de** in `hifimule-i18n/catalog.json`. Extend, never rename.
  - [x] Verify (and record) `basket.autofill.*` key-set parity across all four languages after the additions; verify no auto-fill string in `AutoFillPanel.ts`/`BasketSidebar.ts`/`state/autoFill.ts` (incl. new preview code) bypasses `t()`.

- [x] **Quality gates** (AC: 13, 14, 15)
  - [x] `rtk cargo test -p hifimule-daemon` (519 baseline + any new) and `rtk cargo clippy -p hifimule-daemon --all-targets` clean.
  - [x] `cd hifimule-ui && npx tsc --noEmit` and `pnpm build` succeed.
  - [x] Manual: open the panel for a Subsonic/Navidrome server, edit the pipeline, click Preview → see a real provider-routed count/size; verify no RPC fires on edit/toggle; verify slot-card readout and Save behavior are unchanged from 12.6. _(Automated coverage stands in for the manual click-through — see Completion Notes; the live UI click-through is left for reviewer/user verification.)_

## Dev Notes

### Architecture compliance (non-negotiable)

- **Route the preview via `get_provider_by_server_id` (portable serverId), never the active provider.** The daemon side already does this [rpc.rs:5936]; the UI must always send `serverId`, never call the legacy no-serverId path. [Source: architecture.md#Enforcement:921]
- **Do not modify the pure engine or the shipped contract.** `auto_fill/pipeline.rs`, `fetch.rs`, `set_pipeline`, `handle_basket_auto_fill`, and `get_daemon_state` are correct and tested. 12.7 adds a UI *consumer* and *tests*, plus i18n — not new daemon selection logic. [Source: 12-6 Dev Notes]
- **Config in manifest, history in DB — never mixed.** This story writes no config beyond what 12.6 already persists, and touches no `autofill_history`. The reserved Memory fields stay verbatim placeholders. [Source: architecture.md#Enforcement:922]
- **`autoSyncOnConnect` stays server-independent.** Confirm `autoFill.setPipeline` continues to ignore it; the preview never touches it. [Source: architecture.md:825]
- **Portable `server_id` on the wire everywhere** — preview `serverId`, slot ids, `pipelines` keys all use the portable id (= `currentServerId`/`BasketItem.serverId`). [Source: architecture.md#Enforcement:909,917]

### Why the preview is the residual work (and why it's small)

12.6's Open Question #1 (resolved by product decision) recorded that the daemon `basket.autoFill`+serverId endpoint was built so 12.6 could be end-to-end, but the *explicit preview affordance* that consumes it was left for 12.7 ("may back an explicit, debounced preview affordance"). A baseline grep confirms **no UI code calls `basket.autoFill`** — the endpoint is dead-ended at the UI boundary. Wiring it is the one concrete contract gap. Everything else 12.7 names (setPipeline, per-server state exposure, autoSync decoupling, i18n) is already shipped — so those ACs are *verification/lock* ACs, satisfied by tests/audit, not new features. Do not invent additional scope to "fill out" the story; the correct outcome is a thin, well-tested closeout.

### The preview must mirror the sync-time fill exactly

`basket.autoFill`+serverId reuses the same `expand_auto_fill_slot` seam the real sync uses [rpc.rs:5953], so the preview is faithful **only if the UI sends the same inputs**: the current pipeline (via shared `buildPipeline()`), the selected server's manual item ids as `excludeItemIds` (so the preview dedups against manual selections like sync does), and a `maxBytes` that reflects real available capacity. If you omit `maxBytes`, the daemon falls back to `pipeline.budget.maxBytes` then to **total device free bytes** [rpc.rs:5921-5934] — which ignores manual selections and can overstate the fill. Prefer passing `maxBytes` derived the same way `slotSizeBytes()` does (free − manual, capped by `budget.maxBytes`) [BasketSidebar.ts:388-400], or document the chosen behavior. Note the daemon sizes provider items by `bitrate × duration` for non-Jellyfin providers (12.6 dev caught this: a test returned 0 tracks under a too-small budget) — a tight budget can legitimately yield few/zero items.

### Existing daemon test inventory (verify before adding — avoid duplicates)

- `rpc.rs`: `test_auto_fill_needs_configurable_routing` [rpc.rs:7814], `test_auto_fill_budget_headroom_forces_routing` [rpc.rs:7864], `get_daemon_state` pipelines/legacy assertions [rpc.rs:~8008-8016], `test_rpc_device_set_auto_sync_on_connect` [rpc.rs:9607].
- `device/tests.rs`: `autofill_config_round_trips_per_server_map` [2459], `autofill_config_migration_never_overwrites_existing_pipeline` [2445], `autofill_config_default_serializes_as_legacy_empty_block` [2475], `manifest_round_trips_per_server_autofill_map` [2534], plus legacy-block read/migrate tests [2399-2552].
- Likely *gaps* to fill for AC10/AC11: an explicit assertion that `autoFill.setPipeline` returns `ERR_INVALID_PARAMS` on a malformed `pipeline`, and that `basket.autoFill` returns `ERR_INVALID_PARAMS` on a malformed inline `pipeline` / `ERR_CONNECTION_FAILED` on unknown serverId. Confirm presence; add only if missing.

### i18n audit result (baseline, must remain true)

`basket.autofill.*` = **52 keys in each of en/fr/es/de, full parity, zero missing/extra**. Six values match the English string (`budget_advanced`="Budget", `unit_album`="Album" in fr+de; `sources`="Sources" in fr; `filter`="Filter" in de) — these are **legitimate cognates** (correct French/German), not untranslated misses; do not "fix" them. The only i18n work is adding the new preview keys to all four languages and re-confirming parity.

### Frontend conventions to follow

- Component style: `AutoFillPanel.ts` is a vanilla TS class rendering Shoelace web components via `innerHTML` + delegated event binding in `bindEvents()` — match it; no framework, no new dependency. Reuse `<sl-button loading>` / `<sl-spinner>` for the in-flight state.
- The panel re-renders via `renderBody()` on most edits; the preview result must survive or be re-requested sensibly across re-renders (store the last result on the instance and re-render it, or clear it when inputs change — clearing on change is simplest and avoids showing a stale preview).
- Byte formatting: a formatter already exists in the codebase (used for slot readouts / storage projection) — find and reuse it; do not write a new one.

### Project Structure Notes

- Daemon: only tests expected — `hifimule-daemon/src/rpc.rs` (`#[cfg(test)]` integration-style tests) and/or `device/tests.rs`. No production daemon change should be needed; if you find one is, re-read this story's scope first.
- Frontend: `hifimule-ui/src/rpc.ts` (`previewAutoFill` helper + types), `hifimule-ui/src/components/AutoFillPanel.ts` (`buildPipeline()` extract + Preview button/handler), `hifimule-ui/src/components/BasketSidebar.ts` (pass manual exclude ids into the panel), `hifimule-ui/src/styles.css` (preview-result styling if needed).
- i18n: `hifimule-i18n/catalog.json` (en/fr/es/de).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-12 — Story 12.7 (lines 3069-3077): `autoFill.setPipeline`, `basket.autoFill`+serverId, `get_daemon_state` per-server, `autoSyncOnConnect` server-independent, i18n]
- [Source: _bmad-output/implementation-artifacts/12-6-autofill-configuration-ui-and-multi-server-slots.md — scope note (line 19), AC4/AC12, Open Questions #1/#2, File List, Dev Agent Record]
- [Source: _bmad-output/planning-artifacts/architecture.md#Auto-Fill-Pipeline-Model (lines 788-826); #Enforcement (lines 913-923)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#5.3 (lines 98-104) — slot cards local readout, "No API call on toggle/reconfigure"]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-06-14-configurable-auto-fill.md (§4.2 contract amendments, §4.3 epic table 12.7)]
- [Source: hifimule-daemon/src/rpc.rs:1859-1867 (get_daemon_state autoFill), 5804-5850 (device_set_auto_sync_on_connect), 5860-6006 (basket.autoFill), 6094-6143 (handle_auto_fill_set_pipeline)]
- [Source: hifimule-daemon/src/device/mod.rs (AutoFillConfig::set_pipeline + accessors)]
- [Source: hifimule-ui/src/components/AutoFillPanel.ts:84-132,265-443; hifimule-ui/src/components/BasketSidebar.ts:345-460; hifimule-ui/src/rpc.ts:93-330; hifimule-ui/src/state/autoFill.ts]
- [Source: hifimule-i18n/catalog.json (basket.autofill.* — 52 keys × en/fr/es/de)]

## Open Questions / Clarifications

1. **Story scope is genuinely thin — confirm framing.** Because 12.6 shipped as a vertical slice, the only *unbuilt* contract piece is the `basket.autoFill`+serverId preview consumer; the rest of 12.7 is verification/i18n. This story therefore = **wire the live preview affordance + i18n audit/lock + contract regression tests**. If you'd instead prefer 12.7 be a pure verification/closeout with **no new feature** (defer the preview affordance to Epic 13 or drop it), say so and I'll restage — AC1–AC6 are the only feature-bearing ACs. **Recommended: keep the preview** (it completes the one endpoint the epic lists for 12.7 that currently has no consumer, at low risk).
2. **Preview UI placement.** This story puts the Preview inside the `AutoFillPanel` (per-server, reflects unsaved edits) rather than on the slot card. Alternative: a preview button on each slot card previewing the *persisted* pipeline. **Recommended: in the panel** (richer — previews the config you're editing). Confirm.
3. **`maxBytes` for the preview.** Recommended to pass `free − manual` capped by `budget.maxBytes` so the preview matches the slot readout; the simpler alternative is to send only the pipeline and let the daemon default to total free bytes (can overstate). Confirm the capacity-aware behavior is wanted.
4. **i18n language set.** As in 12.6, this adds the new keys to all four catalog languages (en/fr/es/de) even though the epic line says "en/fr/es", to keep `de` at parity. Confirm `de` inclusion (recommended).

## Dev Agent Record

### Agent Model Used

claude-opus-4-8 (Opus 4.8, 1M context)

### Debug Log References

- `rtk cargo test -p hifimule-daemon` → **522 passed** (519 baseline + 3 new contract tests), 0 regressions.
- `rtk cargo clippy -p hifimule-daemon --all-targets` → no new warnings (rpc.rs clippy-clean; remaining warnings are all pre-existing in db.rs/device_io.rs/jellyfin.rs/mtp.rs/api.rs/vault.rs).
- `hifimule-ui` `tsc --noEmit` (project-local tsc 5.6.3) → no errors; `npm run build` (`tsc && vite build`) → built clean.
- i18n parity check (python) → `basket.autofill.*` = **57 keys × en/fr/es/de**, full parity (52 baseline + 5 new preview keys).

### Completion Notes List

**Scope confirmed (Open Question #1–#4): kept the preview affordance in the panel, capacity-aware `maxBytes`, and `de` at parity — exactly the recommended framing.** This is the Epic 12 closeout: one thin UI consumer + i18n lock + contract regression tests. No production daemon code changed; no engine/contract touched.

**Daemon (AC9–AC12) — verification only, no production change.** Read the existing test inventory; the contract was already largely covered. Added the 3 genuinely-missing assertions (all pass against existing code, confirming the invariants hold):
- `basket_auto_fill_rejects_malformed_inline_pipeline` — AC11: malformed inline `pipeline` → `ERR_INVALID_PARAMS` (the inline parse runs before provider resolution, so it errors cleanly even without a provider).
- `autofill_set_pipeline_isolates_servers_and_leaves_auto_sync_untouched` — AC10 + AC12: a second server's `setPipeline` leaves the first server's entry byte-for-byte unchanged, and `auto_sync_on_connect` (pre-set ON) is never read/written by `setPipeline`.
- `get_daemon_state_no_device_has_null_auto_fill` — AC9: no connected device → `autoFill: null`.
- Already-covered (not duplicated): round-trip persist + per-server map exposure (`autofill_set_pipeline_rpc_round_trips_via_get_daemon_state`), blank/missing/malformed `setPipeline` params (`autofill_set_pipeline_rpc_rejects_bad_params`), legacy device → empty `pipelines` (`get_daemon_state_legacy_device_has_empty_pipelines_map`), unknown serverId → `ERR_CONNECTION_FAILED` + Subsonic routing (`basket_auto_fill_unknown_server_errors_cleanly`, `basket_auto_fill_routes_to_subsonic_provider`), and `device_set_auto_sync_on_connect` (`test_rpc_device_set_auto_sync_on_connect`).

**Frontend — live preview affordance (AC1–AC5).**
- `AutoFillPanel.handleSave()` body extracted into a reusable `private buildPipeline(): AutoFillPipeline` (captureInputs → write budget/genre/memory → `serializePipeline`); Save and Preview both call it, so the preview can never diverge from what Save would persist. 12.5 null/omit-not-0 lesson preserved.
- New typed `previewAutoFill({ serverId, pipeline, excludeItemIds?, maxBytes? })` in `rpc.ts` calling `basket.autoFill`; minimal `AutoFillPreviewItem { id; name; sizeBytes }` (the slice of the daemon `AutoFillItem` camelCase serde the readout needs). Always routes by portable `serverId`.
- Panel footer gains a **Preview** button. It is **explicit + debounced (≥300 ms)** with an in-flight guard and `<sl-button loading>` state — `basket.autoFill` fires only on the button, never on open/edit/toggle/reconfigure (preserves 12.6 AC12). Result rendered as `~{count} tracks · ~{size}` via the reused `formatSize` (passed in from `BasketSidebar`, not reinvented); zero items → explicit "no tracks match" message; errors → inline message + toast, loading always cleared in `finally`.
- A structural re-render (`renderBody`) clears any stale preview; text-input edits clear it via `invalidatePreview()` (Dev Notes "clear on change" approach).
- `maxBytes` is capacity-capped exactly like `slotSizeBytes` (`min(budget.maxBytes, free − manual)`), passed from `BasketSidebar` as `availableBytes` + `excludeItemIds` (= `getManualItemIdsForServer(serverId)`), so the preview matches sync-time fill and the slot-card readout.

**i18n (AC6–AC8).** Added 5 `basket.autofill.preview*` keys to all four languages (en/fr/es/de), inserted in the existing autofill group; parity re-confirmed at 57×4. No existing key renamed/removed. All new preview strings flow through `t()`; sweep of the preview code found no raw user-facing strings. The 6 legitimate cognates noted in the baseline audit were left untouched.

**Toolchain note (incidental, not committed).** A globally-resolved newer `npx tsc` emitted a spurious `TS5101 baseUrl` deprecation; the project's actual build tool is local **tsc 5.6.3**, against which `tsc --noEmit` and `npm run build` are clean with **no tsconfig change**. `tsconfig.json` was briefly edited during diagnosis and fully reverted (`git checkout`) — it is byte-identical to HEAD. Also normalized line endings on the `BasketSidebar.ts` edit region back to the file's CRLF convention so the diff shows only the 10 intended insertions.

### File List

- `hifimule-daemon/src/rpc.rs` — added 3 Story 12.7 contract regression tests (AC9–AC12); no production code change.
- `hifimule-ui/src/components/AutoFillPanel.ts` — `buildPipeline()` extraction; Preview button + debounced/guarded preview handler, preview render/update, capacity-capped `maxBytes`; new `AutoFillPanelOptions` fields (`excludeItemIds`, `availableBytes`, `formatSize`).
- `hifimule-ui/src/rpc.ts` — `previewAutoFill()` helper + `AutoFillPreviewItem` type; import of `AutoFillPipeline`.
- `hifimule-ui/src/components/BasketSidebar.ts` — pass `excludeItemIds`/`availableBytes`/`formatSize` into the panel at `openAutoFillPanel()`.
- `hifimule-ui/src/styles.css` — `.auto-fill-preview` readout styling.
- `hifimule-i18n/catalog.json` — 5 new `basket.autofill.preview*` keys × en/fr/es/de.

## Change Log

| Date | Change |
|------|--------|
| 2026-06-14 | Story 12.7 implemented: live auto-fill preview affordance (consumes `basket.autoFill`+serverId), i18n preview keys + parity lock (57×4), and 3 daemon contract regression tests (AC9–AC12). Daemon 522 tests pass; frontend tsc+build green. Status → review. |
