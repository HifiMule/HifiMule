# Story 17.1: Validate the Audiobookshelf integration contract

Status: ready-for-dev

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

- [ ] Freeze the contract scope, supported-version matrix, and evidence policy (AC: 1–4).
  - [ ] Record the exact Audiobookshelf release/build(s), deployment/auth mode(s), probe date, and source revision. Treat the current upstream OpenAPI document and source behavior as hypotheses to validate, not a substitute for observed supported-server evidence.
  - [ ] Define a fixture manifest/schema version and redaction rules before recording requests or payloads. Version additions additively; never overwrite historical observed behavior as a pass.
  - [ ] Define the status vocabulary: verified, unsupported, ambiguous, blocked, and untested. A missing supported-server observation is not a pass.
- [ ] Probe and record authentication and library-role behavior (AC: 1–3).
  - [ ] Validate login/session or supported bearer/refresh behavior, authorization failures, expiry/refresh/revocation semantics, and safe error classification without persisting secrets.
  - [ ] Validate library discovery, immutable upstream library IDs, and `book` versus `podcast` media type/role behavior; record mixed-library, inaccessible, missing, and changed-library outcomes.
- [ ] Probe and record catalogue, search, metadata, ordering, and identity behavior (AC: 1–3).
  - [ ] Exercise both Books and Podcasts catalogue/search pagination (including boundary/empty/final page behavior); record request parameters, response paging fields, sort/filter assumptions, duplicate/missing item behavior, and typed failures.
  - [ ] For Books, prove multi-part ordering, chapter/file ordering, a single-file book, multiple/missing authors and narrators, artwork availability, and the stable identity fields needed by later mapping/playback work.
  - [ ] Cover changed and removed remote items. Do not infer identity from title, path, index, artwork URL, or a transient playback URL.
- [ ] Probe direct delivery and player-progress contracts (AC: 1–3).
  - [ ] Validate daemon-private direct-stream and transcode request/response behavior, authentication placement, content/type/range behavior where applicable, availability/incompatibility outcomes, and which fields are stable enough for later capability decisions.
  - [ ] Validate book and podcast progress read/write/completion endpoints, whole-item versus chapter/episode semantics, identity requirements, idempotency/conflict/failure behavior, and stale/missing-item handling. This records a contract only; it must not implement progress synchronization.
- [ ] Produce offline, redacted fixtures and deterministic contract validation (AC: 1–3).
  - [ ] Add a versioned fixture directory under `hifimule-daemon/tests/fixtures/audiobookshelf/<supported-version>/` with a README/manifest linking every fixture to a contract case and its redaction status.
  - [ ] Add deterministic fixture/mock validation using the existing Rust `mockito` provider-test pattern. Automated tests must be offline and assert method, path, query/header shape (without secrets), semantic response handling, pagination/order/identity invariants, and error classification.
  - [ ] Include fixtures for auth failure; Books/Podcasts library type; paginated catalogue/search; multi-part and single-file books; multiple/missing author/narrator; artwork; changed/missing item; unavailable/incompatible stream/transcode; progress success/conflict/failure.
- [ ] Publish the authoritative implementation handoff without shipping integration code (AC: 1–4).
  - [ ] Create `docs/audiobookshelf-integration-contract.md` (versioned sections or an adjacent versioned record) containing DTO field authority, transport/auth rules, endpoint observations, supported-version matrix, fixture mapping, failure semantics, and unsupported/ambiguous register.
  - [ ] State the rules inherited by Stories 17.2–17.9: all server traffic remains behind `MediaProvider`; authenticated URLs/tokens stay daemon-side; Books and Podcasts retain separate roles; author is primary and narrator secondary for future book mapping; progress is player-owned only and uses proven stable remote identity plus whole-item offset.
  - [ ] Keep this story discovery-only. Defer `AudiobookshelfProvider`, `ServerType::Audiobookshelf`, factory detection, encrypted-vault/config changes, UI, RPCs, catalogue mapping, direct playback, progress write-back, sync, Autofill, and collection behavior to their dedicated later stories.

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

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created.
- This story is ready for discovery/contract work, not provider implementation.

### File List

- `_bmad-output/implementation-artifacts/17-1-validate-audiobookshelf-integration-contract.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

