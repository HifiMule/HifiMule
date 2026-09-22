---
baseline_commit: a53cb96a78c07e2f8491214fa9163248bc9a0ef4
---

# Story 17.2: Connect Audiobookshelf libraries as independent servers

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a user,
I want to authenticate to Audiobookshelf and select one library for a server,
so that Books and Podcasts libraries become independent HifiMule servers.

## Acceptance Criteria

1. **Explicit provider choice and safe discovery.** The first-run and inline Add Server flows offer Audiobookshelf explicitly; Story 17.1 did not validate an unauthenticated detection endpoint, so `auto` probing must not guess Audiobookshelf. A successful daemon-side local username/password login against the validated Audiobookshelf v2.36.1 contract discovers the accessible libraries through `AudiobookshelfProvider::discover`, not the generic unscoped provider factory. Generic `server.connect` with `serverType: "audiobookshelf"` returns structured `LIBRARY_SELECTION_REQUIRED`; it cannot create an unscoped provider. No server row or vault entry is written until a library is selected and committed.
2. **One-library picker.** After authentication, the UI presents an accessible, keyboard-operable picker containing only safe display data and the role of each discovered library. Upstream `mediaType: "book"` is labeled Books and maps to persisted role `audiobook`; `mediaType: "podcast"` is labeled Podcasts and maps to persisted role `podcast`. The picker selects exactly one library per server creation. It contains no folder, collection, series, or remote-write controls.
3. **Independent durable configuration.** Committing a selection creates one normal Server Hub entry and persists the normalized endpoint, `ServerType::Audiobookshelf`, username, immutable upstream library ID, immutable library role, display identity, server version only when returned by a validated response, machine-local server UUID, and portable server ID. Audiobookshelf-only fields are nullable for existing providers; an Audiobookshelf row is invalid unless both library ID and role are present. The multi-file DB/vault operation is synchronously compensated and crash-reconciled as specified below; it must not claim impossible cross-file transaction atomicity.
4. **Multiple libraries and idempotent upsert.** Multiple Books and/or Podcasts libraries from the same endpoint/account coexist as separate server rows. Reconnecting the same normalized endpoint/account/library upserts that row without changing either its machine-local ID or frozen portable ID; selecting another library inserts a new row. Existing Jellyfin/Subsonic URL-upsert behavior remains unchanged. A saved library must never silently change role; a missing library or role mismatch is reported as stale configuration and leaves stored state unchanged.
5. **Library-scoped portable identity.** A single helper `derive_audiobookshelf_server_id(canonical_url, username, library_id, role)` hashes the exact UTF-8 basis `v2|audiobookshelf|url:{canonical_url}|user:{username}|library:{library_id}|role:{role}` with SHA-256 lowercase hex. `canonical_url` uses existing URL normalization; `username` is UI-trimmed once and then case-preserved; library ID is preserved exactly; role is the lowercase persisted enum slug. Fixed vector: URL `https://abs.example.test`, username `Alexis`, library `lib_books`, role `audiobook` → `319282b9414017e2d8b92213415f590a31ca0293e04fcb2088701f82dc8a7945`. The result is frozen on first persistence. Library display name is not identity. Existing v1 Jellyfin/Subsonic derivation and persisted IDs remain byte-for-byte unchanged. Vault/provider cache stay keyed by local UUID; baskets/manifests use the portable ID.
6. **Encrypted credentials and daemon-only session state.** The existing machine-bound encrypted UUID-keyed vault stores the Audiobookshelf local-auth password for the selected server record; access and refresh JWTs, authorization headers, authenticated URLs, raw provider responses, and temporary discovery state remain daemon-memory-only and never enter SQLite, config files, RPC results, UI state, logs, or fixtures. Restart/lazy connect re-authenticates from the vault and validates the persisted library ID and role before the provider is usable.
7. **Validated auth lifecycle and truthful errors.** The provider sends Bearer auth, refreshes at most once after an access-token-expiry 401, and never loops refresh. On 429 it performs no automatic immediate retry and returns a structured rate-limited error; honor `Retry-After` only when present and valid, otherwise require a later user retry—never hardcode the controlled deployment's 15-second test window. Invalid credentials map to scoped re-auth; inaccessible library 403 is non-retryable; missing library 404 is stale configuration; 5xx remains sanitized with no invented retry promise. Synchronous failure follows the compensation contract below.
8. **Scoped re-authentication.** `server.reauthenticate` targets the affected machine-local server ID, not URL alone, and accepts the password only; it uses the row's stored, case-preserved username. Changing account/username requires new server setup. Re-auth replaces only that row's vault credential after validating the same library ID and role, and retains local/portable IDs, display identity, and selection. This remains unambiguous when several Audiobookshelf rows share an endpoint.
9. **Restart and provider-boundary behavior.** `AudiobookshelfProvider` is constructed behind `MediaProvider`, is lazy-cached by local server ID, and reports `ServerType::Audiobookshelf`. Add provider-neutral `ProviderLibraryRole { Audiobook, Podcast }` and a defaulted `MediaProvider::library_role() -> Option<ProviderLibraryRole>` (`None` for current providers, the persisted role for Audiobookshelf), so later stories never downcast or query DB state to branch by role. In this story catalog, browse/search, artwork, playback, progress, sync, Autofill, playlist/collection, and compatibility operations remain capability-gated or return `UnsupportedCapability`.
10. **Server Hub and localization.** `server.list`/daemon state expose only safe role metadata, not upstream library ID or credentials. Default names use the safe discovered library name plus role (for example `Fiction — Books`) so same-role libraries remain distinguishable; names stay user-editable and non-identifying. First-ever commit becomes selected; adding/upserting while another server is selected does not steal selection; reconnecting the selected row keeps it selected. Server Hub badges/icons distinguish audiobook and podcast roles and reuse rename/remove/select behavior. New strings exist in every catalog locale with visible focus, accessible names, and Shoelace conventions.
11. **Non-regression and evidence.** Offline deterministic tests cover auth/discovery DTOs and error classification, compensated commit failures and crash remnants, same-library upsert, two libraries at one endpoint, role immutability, remove/re-add identity, restart/lazy construction, scoped re-auth, secret redaction, and absence of folder/collection UI. Existing Jellyfin/Subsonic connect/upsert/restart, Server Hub, portable identity, vault migration, and basket behavior remain green.

## Tasks / Subtasks

- [ ] **Implement the Audiobookshelf connection adapter from the validated contract** (AC: 1, 6, 7, 9)
  - [ ] Add `hifimule-daemon/src/providers/audiobookshelf.rs` and register it in `providers/mod.rs`; reuse workspace `reqwest`, `serde`, Tokio, `thiserror`, and dev `mockito` rather than adding an SDK or HTTP-test dependency.
  - [ ] Add `ServerType::Audiobookshelf`, `ServerTypeHint::Audiobookshelf`, slug parsing, `ProviderLibraryRole`, the defaulted trait accessor, capability-safe defaults, and exhaustive enum matches. `AudiobookshelfProvider::discover` is the only unscoped entry; persisted-provider construction requires library ID + role. Keep `auto` probing unchanged.
  - [ ] Make generic `server.connect` reject an Audiobookshelf hint without library setup using structured `LIBRARY_SELECTION_REQUIRED`; preserve its Jellyfin/Subsonic wire behavior.
  - [ ] Implement local login, accessible-library discovery, refresh-once handling, and selected-library validation exactly against `docs/audiobookshelf-integration-contract.md` and the v2.36.1 fixtures. Do not add catalogue mapping or media operations.
  - [ ] Keep provider access/refresh tokens and raw responses private; implement redacted `Debug`/errors and pass all external error text through the existing sanitizer before RPC/log use.

- [ ] **Add a daemon-owned two-stage setup contract** (AC: 1, 2, 6, 7)
  - [ ] Add `server.audiobookshelf.discover({ url, username, password }) -> { setupId, libraries: [{ choiceId, name, role }] }`. Store pending state in a dedicated `AppState` mutex/map. Generate 128-bit IDs from `OsRng`; use a five-minute TTL and maximum eight pending setups; purge expired entries on discover/commit/cancel.
  - [ ] Pending objects are non-`Clone`, redacted in `Debug`, use `secrecy` where practical, and are dropped immediately on expiry/cancel/consume. Under one lock, `server.audiobookshelf.commit` compare-and-removes the setup before persistence, making it one-use under replay/concurrent calls. Any failed commit consumes the setup and requires discovery again.
  - [ ] Add `server.audiobookshelf.cancelSetup({ setupId })`; cancel is idempotent, shutdown clears the map, and tests cover expiry, replay, concurrent commit, and commit-vs-cancel. Never place secrets or upstream IDs in `JsonRpcError.data`.
  - [ ] Add `server.audiobookshelf.commit({ setupId, choiceId, name?, icon? })`; re-check discovered role and use the safe library name as the default display name. Implement synchronous compensation: snapshot any existing row, credential, and selection; precompute local/portable IDs; keep cache unpublished; write/replace the UUID vault entry first; perform the DB upsert in one SQLite transaction; then publish manager/cache/selection. If a synchronous step fails, restore the prior vault snapshot or remove the new entry, roll back DB, and leave cache/selection unchanged.
  - [ ] On startup, treat a DB row with no vault credential as configured-but-reauth-required: list it, never provider-cache it, and surface scoped re-auth on use. Orphan vault entries are ignored and may be cleaned safely. This is crash reconciliation, not a claim of cross-file process-crash atomicity.
  - [ ] Return structured, localizable error data for expired setup, empty accessible-library list, invalid choice, authentication failure, forbidden/missing library, rate limit, and sanitized connection failure.
  - [ ] Register discover/commit/cancel and `server.reauthenticate` consistently in RPC dispatch and mutation classification; discovery does not persist app state but is credential-sensitive, while commit/cancel/reauth mutate daemon state.

- [ ] **Persist immutable library scope without breaking existing servers** (AC: 3–5)
  - [ ] Extend `ServerConfig`/`ServerRecord` and the `server_config` table with nullable `provider_library_id` and `provider_library_role`; use an additive idempotent migration and update every SELECT/row mapper/test helper in lockstep.
  - [ ] Define a typed `AudiobookshelfLibraryRole` (`audiobook`, `podcast`) and reject incomplete/unknown/mismatched Audiobookshelf configurations. Keep both fields `NULL` for Jellyfin/Subsonic.
  - [ ] Refactor `upsert_server` (or add a scoped variant) so Audiobookshelf matches `(normalized endpoint, username, provider_library_id)` while legacy providers retain their existing URL-only rule. Freeze library ID and role after insert; a different library is never an update.
  - [ ] Implement and test the normative `derive_audiobookshelf_server_id` basis/vector from AC5. Preserve all v1 inputs/outputs and freeze-on-update behavior; do not derive identity from library name or index.
  - [ ] Update default label/icon helpers and server JSON mapping. Expose role and safe display name to the UI, but keep upstream library ID daemon-private. Store server version as `None` unless an already validated response supplies it; never infer `2.36.1` from payload shape.
  - [ ] Test selection semantics: first server auto-selects; adding/upserting another while a server is selected preserves selection; reconnecting the selected row keeps selection.

- [ ] **Integrate vault, restart, cache, and scoped re-authentication** (AC: 6–9)
  - [ ] Store the local-auth password in the existing encrypted `ServerCredentials.token_or_password` entry keyed by local UUID. Do not alter `vault.rs`, re-key the vault, overload `user_id`, or persist JWTs.
  - [ ] Extend `server_manager::connect_provider_for` to reconstruct an Audiobookshelf provider from the saved row/vault entry, authenticate lazily, and verify the exact library ID/role before caching it.
  - [ ] Add `server.reauthenticate({ id, password })`. Use the saved username, library ID, and role; update only that vault entry after validation, evict/replace only that cached provider, and preserve identities and selection. A username/account change goes through new setup.
  - [ ] Audit logout/remove/select/daemon-state paths and all `ServerType` matches so Audiobookshelf follows established local-ID cache/vault semantics without changing Jellyfin/Subsonic behavior.

- [ ] **Build the explicit provider and library-picker UX** (AC: 1, 2, 8, 10)
  - [ ] Extend `login.ts` with a provider selector; preserve first-run, inline Add Server, and re-auth modes. Audiobookshelf submission enters discovery/picker instead of immediately calling generic `server.connect`.
  - [ ] Render Books/Podcasts choices with role labels, loading/empty/error states, keyboard navigation, visible focus, cancel/back behavior, and one clear commit action. Do not render folder, collection, series, or multi-library checkboxes.
  - [ ] Update re-auth callers to pass the affected local server ID and use the scoped RPC; URL-only re-auth is forbidden for Audiobookshelf because several rows may share that URL.
  - [ ] Extend `rpc.ts`, `serverIdentity.ts`, and Server Hub summaries with the safe role contract. Add role-aware default labels/icons while retaining user rename/icon overrides.
  - [ ] Stop logging sensitive RPC parameters: replace the current `console.log('RPC Call:', method, params)` behavior with method-only or explicit field-level redaction for every credential-bearing RPC.
  - [ ] Add every visible string to `hifimule-i18n/catalog.json` for all existing locales and regenerate/update `hifimule-ui/src/i18n-catalog.d.ts` using the project convention.

- [ ] **Add deterministic contract, persistence, RPC, and UI regression coverage** (AC: 1–11)
  - [ ] Reuse the Story 17.1 v2.36.1 fixture corpus and `mockito` conventions to test login/Bearer shape, refresh exactly once, discovery mapping, 401/403/404/429/5xx classification, and redaction. Tests must be offline and must not turn an unobserved case into a pass.
  - [ ] Add DB tests for additive migration; two same-endpoint libraries; same-library upsert; immutable role/library scope; distinct deterministic portable IDs; frozen IDs on update; remove/re-add stability; and unchanged Jellyfin/Subsonic v1 IDs.
  - [ ] Add RPC/manager tests for no-write discovery, compensated commit failure and crash reconciliation, safe response shapes, lazy restart validation, local-ID-scoped re-auth, provider-cache isolation, remove/select behavior, and no secret/upstream-ID leakage.
  - [ ] Without adding a UI test framework, extract pure setup/picker/redaction helpers and test them with the repository's existing Node `node:test` + TypeScript-transpile pattern. Cover provider selection, roles, setup expiry/cancel/failure preservation, multi-library cards, re-auth targeting, logging redaction, and absence of folder/collection controls; record a manual keyboard/focus check for the Shoelace dialog.
  - [ ] Run `rtk npm run build:daemon -- test -p hifimule-daemon`, the targeted `node --test` UI files, `rtk npm run build:ui`, formatting, and `rtk git diff --check`. Report unavailable environment checks truthfully.

## Dev Notes

### Scope and source precedence

- The final Epic 17 text and sprint key are authoritative. The original 2026-09-20 change proposal described Story 17.2 as Books-only, but the final `epics.md`, PRD FR82, architecture/UX amendments, sprint tracker, and completed Story 17.1 require both Books and Podcasts library configuration here. Story 17.7 still owns podcast catalogue mapping, browsing, and playback.
- Story 17.1 is the mandatory completed gate. Its v2.36.1 evidence is implementation authority; upstream OpenAPI/master or older hosted API prose is supporting context, not permission to invent behavior.
- This story is connection/configuration only. Stories 17.3–17.9 own catalog mapping, browse/search, artwork delivery, direct playback, progress, device sync, Autofill, read-only series/collections, and compatibility feedback.

### Current state and required changes

| File / seam | Current state | Story 17.2 change | Must preserve |
| --- | --- | --- | --- |
| `hifimule-daemon/src/providers/mod.rs` | `MediaProvider`, Jellyfin/Subsonic factories, hints/types/slugs, URL probe, shared sanitizer; no provider role. | Register the adapter/type, defaulted role accessor, discovery entry, and scoped persisted construction; keep later media methods unsupported. | Provider boundary, existing detection order, error redaction, Jellyfin/Subsonic behavior. |
| `hifimule-daemon/src/db.rs` | `server_config` has no library scope; upsert matches normalized URL only; portable v1 identity is type + reported-ID/URL + username and is frozen. | Add nullable library ID/role, library-scoped ABS upsert and v2 portable basis. | Existing rows/IDs, inline idempotent migrations, selection/order, v1 derivation. |
| `hifimule-daemon/src/server_manager.rs` | Rows and lazy cache are keyed by local UUID; stored construction supports Jellyfin/Subsonic. | Carry library scope and reconstruct/validate ABS lazily. | One cache keyed by local UUID and portable-to-local routing. |
| `hifimule-daemon/src/rpc.rs` | `server.connect` is one-stage and URL-upserts; re-auth reuses URL; JSON summaries lack role. | Add named discover/commit/cancel and local-ID re-auth contracts, pending setup state, synchronous compensation, and crash reconciliation. | Existing RPCs/error envelope; current selected server/basket on add failure. |
| `hifimule-daemon/src/api.rs` / `vault.rs` | Encrypted per-local-ID `ServerCredentials`; vault crypto and migrations already exist. | Reuse the existing password slot; no JWT persistence. | Encryption format, other servers' credentials, test lock/seams. |
| `hifimule-ui/src/login.ts` | Auto probe + one-step generic credentials form; inline add and URL-only re-auth. | Explicit provider choice, two-stage picker, scoped re-auth. | First-run/add/reauth modes, current server on inline add, Shoelace patterns. |
| `hifimule-ui/src/rpc.ts` | Generic RPC wrapper logs full params; `ServerSummary` lacks role. | Add typed safe contracts/role and redact credential-bearing logs. | Structured `RpcError`, unauthorized event behavior. |
| `hifimule-ui/src/serverIdentity.ts` / `components/ServerHub.ts` | Provider labels/icons and reusable server cards for current types. | Add Audiobookshelf role-aware presentation using existing cards. | User identity overrides and select/edit/remove interactions. |
| `hifimule-ui/src/main.ts` | Re-auth prompt identifies the server by URL. | Carry the machine-local server ID into scoped re-auth. | Central unauthorized-event flow and selected-server refresh. |
| `hifimule-i18n/catalog.json` | Four-locale catalog with typed generated keys. | Add all new setup/role/error strings in every locale. | No hardcoded user-facing English; key parity. |

### Architecture and identity guardrails

- **Do not reuse URL-only upsert for Audiobookshelf.** It would overwrite the first selected library when a second library is added from the same endpoint.
- **Do not reuse the v1 URL-based portable identity unchanged.** It would give all libraries for the same endpoint/account the same portable ID and corrupt Server Hub, basket, manifest, and routing semantics.
- Local UUID remains the DB primary key, encrypted-vault key, provider-cache key, and key for `server.select/remove/update/reauth`. Portable ID remains the basket/manifest/sync-routing identity. Do not cross these roles.
- The real upstream library ID is daemon data. The UI chooses an opaque setup choice ID and receives only role/display metadata. Library names, titles, paths, ordinals, artwork URLs, and transient URLs are not identity.
- Preserve identity on re-auth and ordinary upsert. If a user intentionally chooses another library, create another server record; never mutate the saved library/role of an existing row.
- The provider-neutral role accessor is the only runtime role seam. Do not downcast `Arc<dyn MediaProvider>` or make later catalogue code query `server_config` to decide whether it is handling audiobooks or podcasts.

### Authentication, security, and error guardrails

- Supported behavior is local username/password authentication on Audiobookshelf v2.36.1. Official Audiobookshelf documentation says API keys are for automation and third-party apps should use the standard JWT flow; do not add API-key or OIDC UI in this story without a separately validated contract.
- Keep access/refresh tokens in the provider or pending setup object only. Restart uses the encrypted password to obtain a new session. Refresh once after an expiry 401; do not recurse or retry 401 indefinitely. Logout revokes refresh but an issued access token may remain valid until expiry, so dropping local session state is still required.
- Treat 401 as authentication, 403 as permission, 404 as stale/missing library, and 429 as rate-limited. Do not hardcode the controlled probe's configured window; use a valid `Retry-After` if supplied or wait for an explicit later user retry. The controlled 500 evidence established classification only, not a retry strategy. Never surface provider response bodies.
- `sanitize_secret_message` is the shared boundary. Also remove the UI's full-param RPC logging, which currently logs passwords before the daemon can sanitize them.
- SQLite and `secrets.enc` cannot share a transaction. Follow the ordered vault-first/DB-transaction/cache-publish protocol in the tasks, compensate every synchronous failure, and reconcile missing-credential/orphan-vault crash remnants at startup. Do not describe this as process-crash atomicity without a journal.
- Secret-bearing pending/provider objects must have redacted `Debug`, avoid `Clone` unless justified, and leave no secrets in error `data`. Drop them promptly on expiry/cancel/consume; do not promise guaranteed zeroization beyond the types actually used.

### Library/framework requirements

- Workspace baseline: Rust 1.93.0, edition 2024; `reqwest` ~0.12, `serde`/`serde_json` ~1, Tokio ~1.49, `thiserror` ~2, `async-trait` ~0.1, `secrecy` 0.8, `uuid` 1, SHA-256 via `sha2` 0.10, and `mockito` 1.5 are already available. UI uses TypeScript ~5.6, Tauri API ~2.10, Vite 6, and Shoelace 2.19. Add no provider SDK or duplicate HTTP/state library.
- As of 2026-09-22, the official Audiobookshelf releases page still marks v2.36.1 latest, matching the validated fixture record. Pin support claims to the observed contract rather than silently widening to future versions. [Official releases](https://github.com/advplyr/audiobookshelf/releases)
- The generated upstream OpenAPI document declares Bearer security and library endpoints, but Story 17.1's controlled observations remain the authority where docs and runtime differ. [Generated OpenAPI](https://github.com/advplyr/audiobookshelf/blob/master/docs/openapi.json)
- The legacy hosted API reference explicitly says it is out of date. Do not use its query-token examples or field shapes over the validated contract. [Legacy API reference](https://api.audiobookshelf.org/)
- Official API-key guidance states that API keys are intended for server-to-server automation while third-party apps should use the standard JWT authentication flow. [API-key guidance](https://audiobookshelf.org/docs/documentation/server-management/api-keys/)

### Testing requirements

1. Provider tests are offline and fixture-backed. Assert method/path/header semantics without real secrets; parse all relevant fixtures and assert full role/error invariants rather than only fixture counts.
2. Identity tests must prove two libraries at one endpoint have distinct local and portable IDs; the same logical library deterministically reuses its portable ID across restart/remove-readd; v1 identities do not change.
3. Failure tests inject DB/vault/cache errors and prove synchronous compensation restores prior state; startup tests cover a DB row missing its credential and an orphan vault entry without pretending cross-file crash atomicity.
4. Security tests inspect RPC payloads/results, `Debug`, tracing output seams, browser logging helpers, SQLite/config/vault test doubles, and fixtures for password/token/header/authenticated-URL/provider-body leakage.
5. Node tests cover extracted pure setup/picker/redaction logic; manual evidence proves Shoelace focus/keyboard behavior, loading/empty/error states, scoped password-only re-auth, and the absence of folder/collection controls.
6. Regression tests keep Jellyfin/Subsonic connect, auto-detection, upsert, portable identity, restart, Server Hub, and vault migration behavior unchanged.

### Previous Story Intelligence

- Story 17.1 shipped no production integration surface. Reuse its contract document, offline fixture test, and `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/` corpus; do not mistake fixture validation for provider-response coverage.
- Review commit `a53cb96` corrected five patterns that remain binding: parse every manifest fixture; distinguish contract tests from runtime adapter tests; assert complete sequences/invariants; leave unobserved cases explicitly undetermined; and use one pinned source revision.
- The contract proved both library roles, immutable library IDs, restricted-library 403, missing-library 404, access expiry/one refresh, login 429, and sanitized error handling. Search pagination, catalogue mapping, delivery, and progress behavior belong to later stories.

### Git Intelligence Summary

- `a53cb96` (`Review 17.1`) corrected fixtures/contract tests and added Books/Podcasts search evidence; no production provider code.
- `564f9e8` (`Dev 17.1`) added the contract, offline validator, and v2.36.1 fixture corpus; no production provider/config/UI/RPC code.
- `c0a64ea` (`Story 17.1`) created the prior story and tracker update.
- `6bb6ace` (`Plan audiobookshelf`) amended planning artifacts only. Production currently has no `AudiobookshelfProvider`, enum/factory path, library-scoped schema, picker, or RPC.

### Project Structure Notes

- New file: `hifimule-daemon/src/providers/audiobookshelf.rs`.
- Expected updates: `hifimule-daemon/src/{providers/mod.rs,db.rs,server_manager.rs,rpc.rs,api.rs}`; `hifimule-ui/src/{login.ts,rpc.ts,serverIdentity.ts,main.ts}`; `hifimule-ui/src/components/ServerHub.ts`; `hifimule-i18n/catalog.json`; generated UI i18n typings; tests/fixtures only where the existing v2.36.1 corpus needs additive request/response cases.
- Avoid changing `hifimule-daemon/src/domain/models.rs` merely to represent setup choices; use provider/setup-specific DTOs so existing browse contracts do not churn before Stories 17.3/17.7.
- CSS changes are optional; reuse existing Shoelace/dialog/input styles unless the picker demonstrably needs a focused, scoped addition.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 17 and Story 17.2]
- [Source: `_bmad-output/planning-artifacts/prd.md` — FR82 and Audiobookshelf non-goals]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Epic 17 Audiobookshelf Architecture Amendment; Multi-Server Management; Portable Server Identity]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — §8 Audiobookshelf integration]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-20-audiobookshelf.md` — impact, sequencing, UX, and non-goals]
- [Source: `_bmad-output/implementation-artifacts/17-1-validate-audiobookshelf-integration-contract.md` — completed gate and review learnings]
- [Source: `docs/audiobookshelf-integration-contract.md` — v2.36.1 observed auth/library/failure contract]
- [Source: `hifimule-daemon/src/providers/mod.rs` — provider boundary, factory, types, sanitizer]
- [Source: `hifimule-daemon/src/db.rs` — schema, URL upsert, v1 portable identity]
- [Source: `hifimule-daemon/src/server_manager.rs` — lazy local-ID cache and stored provider construction]
- [Source: `hifimule-daemon/src/rpc.rs` — server connect/list/select/remove/re-auth and error contracts]
- [Source: `hifimule-daemon/src/api.rs` — encrypted UUID-keyed credential vault]
- [Source: `hifimule-ui/src/login.ts` — first-run/add/re-auth flow]
- [Source: `hifimule-ui/src/rpc.ts` — RPC wrapper, unauthorized event, sensitive-param logging risk]
- [Source: `hifimule-ui/src/serverIdentity.ts` and `hifimule-ui/src/components/ServerHub.ts` — reusable server presentation]
- [Source: `hifimule-i18n/catalog.json` — all-locale string catalog]
- [Source: `hifimule-daemon/tests/audiobookshelf_contract.rs` and `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/` — fixture schema and offline evidence]

## Dev Agent Record

### Agent Model Used

Story preparation: Codex.

### Debug Log References

- 2026-09-22: Story preparation analyzed final Epic 17 requirements, PRD/architecture/UX amendments, completed Story 17.1 and its validated v2.36.1 contract, existing provider/server/vault/UI seams, recent git history, and official upstream release/API/authentication references.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- Story 17.2 is limited to authenticated library discovery and durable independent server configuration; later Audiobookshelf catalogue, playback, progress, sync, Autofill, and collection behavior remain deferred to their owning stories.

### File List

- `_bmad-output/implementation-artifacts/17-2-connect-audiobookshelf-libraries-as-independent-servers.md`
