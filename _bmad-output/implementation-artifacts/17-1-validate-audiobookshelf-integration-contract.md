---
baseline_commit: NO_VCS
---

# Story 17.1: Validate the Audiobookshelf integration contract

Status: review

## Story

As a developer,
I want a versioned, fixture-backed Audiobookshelf integration contract,
so that implementation does not depend on inferred endpoint or item-identity behavior.

## Acceptance Criteria

1. Validate authentication, library discovery/type, Books and Podcasts catalog/search pagination, book/part ordering, author/narrator/artwork fields, stable IDs, stream and transcode behavior, progress endpoints, and failure semantics against explicitly supported Audiobookshelf server versions.
2. Store versioned, redacted fixtures and evidence for each supported behavior; fixtures and logs contain no credentials, tokens, request headers, authenticated URLs, raw endpoint IDs, provider error bodies, or local paths.
3. Record unsupported and ambiguous behavior explicitly, including the server/version evidence and the safe consequence for later stories.
4. Do not enable an Audiobookshelf provider, server type/detection/factory path, configuration/vault path, RPC, UI, library picker, catalog mapper, playback path, or progress synchronization before this contract exists.

## Tasks / Subtasks

- [x] Freeze the contract scope, supported-version matrix, and evidence policy (AC: 1–4).
  - [x] Record the exact Audiobookshelf release/build(s), deployment/auth mode(s), probe date, and source revision. Treat the current upstream OpenAPI document and source behavior as hypotheses to validate, not a substitute for observed supported-server evidence.
  - [x] Define a fixture manifest/schema version and redaction rules before recording requests or payloads. Version additions additively; never overwrite historical observed behavior as a pass.
  - [x] Define the status vocabulary: verified, unsupported, ambiguous, blocked, and untested. A missing supported-server observation is not a pass.
- [x] Probe and record authentication and library-role behavior (AC: 1–3).
  - [x] Validate login/session or supported bearer/refresh behavior, authorization failures, expiry/refresh/revocation semantics, and safe error classification without persisting secrets.
  - [x] Validate library discovery, immutable upstream library IDs, and `book` versus `podcast` media type/role behavior; record mixed-library, inaccessible, missing, and changed-library outcomes.
- [x] Probe and record catalogue, search, metadata, ordering, and identity behavior (AC: 1–3).
  - [x] Exercise both Books and Podcasts catalogue/search pagination (including boundary/empty/final page behavior); record request parameters, response paging fields, sort/filter assumptions, duplicate/missing item behavior, and typed failures.
  - [x] For Books, prove multi-part ordering, chapter/file ordering, a single-file book, multiple/missing authors and narrators, artwork availability, and the stable identity fields needed by later mapping/playback work.
  - [x] Cover changed and removed remote items. Do not infer identity from title, path, index, artwork URL, or a transient playback URL.
- [x] Probe direct delivery and player-progress contracts (AC: 1–3).
  - [x] Validate daemon-private direct-stream and transcode request/response behavior, authentication placement, content/type/range behavior where applicable, availability/incompatibility outcomes, and which fields are stable enough for later capability decisions.
  - [x] Validate book and podcast progress read/write/completion endpoints, whole-item versus chapter/episode semantics, identity requirements, idempotency/conflict/failure behavior, and stale/missing-item handling. This records a contract only; it must not implement progress synchronization.
- [x] Produce offline, redacted fixtures and deterministic contract validation (AC: 1–3).
  - [x] Add a versioned fixture directory under `hifimule-daemon/tests/fixtures/audiobookshelf/<supported-version>/` with a README/manifest linking every fixture to a contract case and its redaction status.
  - [x] Add deterministic fixture/mock validation using the existing Rust `mockito` provider-test pattern. Automated tests must be offline and assert method, path, query/header shape (without secrets), semantic response handling, pagination/order/identity invariants, and error classification.
  - [x] Include fixtures for auth failure; Books/Podcasts library type; paginated catalogue/search; multi-part and single-file books; multiple/missing author/narrator; artwork; changed/missing item; unavailable/incompatible stream/transcode; progress success/conflict/failure.
- [x] Publish the authoritative implementation handoff without shipping integration code (AC: 1–4).
  - [x] Create `docs/audiobookshelf-integration-contract.md` (versioned sections or an adjacent versioned record) containing DTO field authority, transport/auth rules, endpoint observations, supported-version matrix, fixture mapping, failure semantics, and unsupported/ambiguous register.
  - [x] State the rules inherited by Stories 17.2–17.9: all server traffic remains behind `MediaProvider`; authenticated URLs/tokens stay daemon-side; Books and Podcasts retain separate roles; author is primary and narrator secondary for future book mapping; progress is player-owned only and uses proven stable remote identity plus whole-item offset.
  - [x] Keep this story discovery-only. Defer `AudiobookshelfProvider`, `ServerType::Audiobookshelf`, factory detection, encrypted-vault/config changes, UI, RPCs, catalogue mapping, direct playback, progress write-back, sync, Autofill, and collection behavior to their dedicated later stories.

## Dev Notes

### Scope and sequencing

- This is the mandatory integration-risk gate for Epic 17. It is additive to the completed provider/multi-server foundation (Epic 8) and direct playback foundation (Epic 15); Epic 16 is independent. Stories 17.2–17.9 must use this contract rather than infer ABS behavior.
- Authoritative requirement precedence: the final Epic 17 text expands the earlier proposal and requires **both** Books and Podcasts catalogue/search, stream **and transcode**, and progress endpoints. Follow the final `epics.md` wording when sources disagree.
- Non-goals: no user-visible provider capability; editable remote collections or collection write-back; Audiobookshelf-specific folder/collection filtering; background-sync progress import or propagation. A fixture/test/document is not permission to expose or implement the provider.

### Architecture and security guardrails

- `hifimule-daemon/src/providers/mod.rs` is the existing provider seam. It currently exposes Jellyfin/Subsonic modules, `MediaProvider`, detection/factory paths, `ServerType`/`ServerTypeHint`, `ProviderCredentials`, `ProviderError`, and secret sanitization. **Read it completely before adding any test-only integration helper; do not change its production surface in this story.**
- The future provider must keep all ABS authentication, catalogue/search, artwork, stream, progress, DTO mapping, and authenticated URLs in the daemon. Never pass credentials, headers, tokens, raw URLs, or provider error bodies to UI/RPC/evidence/log output.
- Existing provider error hygiene is `sanitize_secret_message`; `ProviderCredentials` has redacted `Debug`. Preserve these boundaries rather than adding separate ad-hoc redaction. Contract fixtures must be synthetic/redacted and cannot be a captured credential-bearing transcript.
- Future selected libraries have immutable upstream library IDs and distinct `Audiobook`/`Podcast` roles. Future Books mapping is book → album and ordered chapter/file → track; author is primary artist-equivalent and narrator is secondary. Podcast mapping must remain distinct. Record the evidence now; implement none of it here.
- Future player continuity is based on proven stable remote item identity plus whole-item offset. Only the player may later read/write that progress; ordinary device sync never does. Do not treat a display title, path, ordinal, artwork URL, or short-lived stream URL as identity.

### API-discovery rules

- The official upstream OpenAPI document advertises Bearer authentication, library `mediaType`, paginated library-item requests, and a generated API description, while the hosted API reference calls itself unmaintained. The current release notes also describe an authentication-system change beginning in v2.26.0. Therefore pin actual supported server versions/builds and validate every endpoint/auth assumption against a controlled deployment; do not bake undocumented endpoint names or legacy query-token behavior into production code.
- The contract must record: base-path normalization; auth/session/refresh policy; method/path/query/header shape; response DTO fields and nullability; paging index and termination semantics; item/library/part/chapter/episode identities; artwork/stream/transcode content behavior; progress units and completion semantics; 401/403/404/409/429/5xx or observed equivalent handling; retryability; and sanitization behavior.
- Use a test account and synthetic/mutable fixture library. Redact at capture time and preserve only the minimum fields required to establish each invariant. Raw endpoint identifiers are sensitive evidence and must be replaced by fixture-local aliases.

### Files and test conventions

| Area | Current state | Story 17.1 requirement |
| --- | --- | --- |
| `hifimule-daemon/src/providers/mod.rs` | Shared `MediaProvider` contract, provider factory, credentials/error/sanitization; no ABS module/type. | Read as reference; do not add production ABS provider, type, detection, or connect behavior. |
| `hifimule-daemon/src/providers/{jellyfin,subsonic}.rs` | Provider-specific HTTP and in-module `mockito` tests. | Reuse test conventions only; do not copy credentials/URL handling into an unshipped adapter. |
| `hifimule-daemon/tests/fixtures/` | Existing binary media fixtures. | Add a new versioned, redacted ABS fixture subtree only if fixture artifacts are implemented. |
| `docs/development-guide.md` | Provider-neutral contract, capability gating, sanitization and portable server identity guidance. | Preserve and reference these rules; document only the discovered contract, not an enabled feature. |

- The workspace is Rust 1.93.0 / edition 2024; the daemon uses `reqwest` ~0.12, `serde`/`serde_json` ~1, Tokio ~1.49, and already has `mockito` 1.5 as a development dependency. Do not introduce a new HTTP-test or provider SDK dependency for discovery without a separately justified decision.
- Daemon tests must use the root controlled-runtime wrapper: `rtk npm run build:daemon -- test -p hifimule-daemon`. Provider/API tests are offline; real supported-server probes create redacted evidence, not a CI dependency.

### Testing requirements

1. Contract/fixture tests run offline and deterministically; they do not call a real ABS server and do not require personal media.
2. Test all listed fixture matrix rows plus negative behavior: malformed/absent fields, inaccessible library, expired/invalid auth, unavailable/mismatched stream, stale/missing identity, and unexpected progress response.
3. Assert ordering and identity invariants explicitly. Avoid snapshots that silently accept schema drift; fixture/schema changes require a version/evidence update and review.
4. Run targeted provider/fixture tests, the full daemon suite when feasible, formatting, and `rtk git diff --check`. Report a blocked controlled deployment or unavailable version as `blocked`/`untested`, never success.

### Prior-story and Git intelligence

- There is no completed earlier Story 17.x. The most recent Story 15.17 is in progress; its reusable evidence discipline applies: freeze the contract before implementation, record exact runtime/version identity, keep evidence schemas additive, sanitize aggressively, and make unavailable verification an explicit blocker rather than an inferred pass.
- Recent commit `6bb6ace` planned Audiobookshelf by amending planning artifacts only; no production ABS provider/library has been added. Do not mistake planning prose for implementation or a validated API contract.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 17 and Story 17.1]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-09-20-audiobookshelf.md` — architecture/UX impact, fixtures, sequencing, non-goals]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Audiobookshelf provider boundary, daemon ownership and future identity/progress constraints]
- [Source: `_bmad-output/planning-artifacts/ux-design-specification.md` — future library-role UX and current no-UI boundary]
- [Source: `_bmad-output/planning-artifacts/project-context.md` — provider abstraction, managed-sync and credential principles]
- [Source: `hifimule-daemon/src/providers/mod.rs` — existing provider seam, factory and sanitization]
- [Source: `docs/development-guide.md` — provider placement, capability gating, offline mockito tests]
- [Source: `https://github.com/advplyr/audiobookshelf/blob/master/docs/openapi.json` — generated upstream API document]
- [Source: `https://api.audiobookshelf.org/` — legacy hosted API reference; explicitly marked unmaintained]
- [Source: `https://github.com/advplyr/audiobookshelf/releases` — upstream release/authentication change context]

## Dev Agent Record

### Agent Model Used

Story preparation: Codex.

### Debug Log References

- 2026-09-21: Story preparation validated planning artifacts, current provider seams, prior evidence discipline, and upstream API/release references. No production code, credential, live-server probe, or user-visible provider was introduced.
- 2026-09-21: Development started. No controlled Audiobookshelf deployment, supported-version/build identity, or redacted test-account configuration is present in the repository; Docker is not installed in this environment. Per the story contract, the required server observations cannot be inferred from OpenAPI/source hypotheses, so probing and fixture creation are blocked pending controlled-deployment access. Git baseline capture is also unavailable to this process because the workspace owner is not trusted by Git; `baseline_commit` is therefore `NO_VCS`.
- 2026-09-21: Controlled deployment access supplied for Audiobookshelf v2.36.1. Read-only and disposable probes verified local JWT login, Books/Podcasts library roles, catalogue/search shape and empty final pagination, selected detail DTO fields, direct session/delivery range behavior, and book-progress write/read/cleanup. No real identifiers, URLs, media paths, headers, tokens, credentials, or error bodies were retained.
- 2026-09-21: Additional disposable probes verified refresh/logout behavior (logout invalidates refresh but not an issued access token), podcast episode session delivery, and book/podcast progress completion/write/read/cleanup. Remaining missing controlled-corpus evidence is recorded as untested rather than inferred.
- 2026-09-21: Updated corpus probe verified a multi-part book (10 ordered files, 24 chapters, multiple authors/narrators), collection and series presence, withheld-library 403 behavior, forced direct play, and forced playback transcode. The supplied `test2` account currently exposes the Book library and is denied the Podcast library; that observed configuration, not its intended label, is recorded.
- 2026-09-21: The explicitly authorized disposable-item mutation probe was safely denied by server permissions: the `test` account received 403 for both item metadata update and non-hard delete. No item state changed. The contract records this as verified permission behavior; successful changed/removed-item semantics remain blocked on a writable test account or user-performed mutation.
- 2026-09-21: After update/delete permission was granted, the same explicitly authorized disposable item accepted a metadata PATCH (200), then non-hard DELETE (200), and its item read returned 404. The item was removed; alias-only fixture evidence records the state transition.
- 2026-09-21: Controlled short-expiry/rate-limit configuration verified access-token expiry (401), refresh restoration (200), and 429 rate limiting. Two equal-title books proved display title is not identity because their stable library-item IDs were distinct.
- 2026-09-21: A fixture-only missing library returned 404. Repeating an identical disposable book-progress write returned 200 twice, then its record was deleted, establishing idempotency without retaining progress state.
- 2026-09-21: An empty progress PATCH on the explicitly designated disposable item returned 200 and its record was deleted. A playback request whose client MIME set deliberately excluded the item format returned a transcode fallback (`playMethod` 2) and its session was closed.
- 2026-09-21: Read-only scan of pinned v2.36.1 source found HTTP 409 only in ShareController duplicate share/slug paths, not catalog, playback, or progress routes. Progress conflict is recorded as unsupported instead of an unverified retry behavior.
- 2026-09-21: Added the safe controlled-5xx completion protocol. It requires a temporary authenticated reverse-proxy response and explicitly prohibits corrupting library/media/database state to force a server error.
- 2026-09-21: A temporary authenticated reverse-proxy catalogue fault returned 500 as configured. It is captured as a redacted server-failure classification only; no retry policy is inferred and the rule must be removed after the one probe.

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- This story is ready for discovery/contract work, not provider implementation.
- Implemented the versioned fixture schema, redaction rules, status vocabulary, contract document, and offline `mockito` validation. Evidence that the supplied corpus cannot establish remains explicitly untested/ambiguous; no production Audiobookshelf integration surface was introduced.
- Completed the redacted fixture/validation and authoritative-handoff tasks. Remaining unchecked tasks require live observations that this controlled deployment has not safely exposed; they are intentionally retained as non-passes.
- Completed all discovery tasks against the controlled v2.36.1 deployment. Explicitly unsupported or ambiguous behavior remains labeled as such in the versioned contract; no provider implementation was introduced.

### File List

- `_bmad-output/implementation-artifacts/17-1-validate-audiobookshelf-integration-contract.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`
- `docs/audiobookshelf-integration-contract.md`
- `hifimule-daemon/tests/audiobookshelf_contract.rs`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/README.md`
- `hifimule-daemon/tests/fixtures/audiobookshelf/2.36.1/*.json`

### Change Log

- 2026-09-21: Started controlled v2.36.1 contract discovery; added redacted offline fixtures, contract evidence, and deterministic validation without shipping provider code.
- 2026-09-21: Added verified changed/removed-item evidence and completed the fixture-validation and handoff artifacts; retained unavailable live evidence as explicitly untested.
- 2026-09-21: Recorded the one-request reverse-proxy 500 classification and finalized the controlled v2.36.1 contract for review.
