---
title: Podcast recent episodes tab
type: feature
created: 2026-09-26
status: done
baseline_commit: 378153ba96157592c8ec6f47faab755967e9172e
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** The podcast library only starts with a show catalog, making newly published episodes across subscriptions hard to find.

**Approach:** Add a “Recent Episodes” tab alongside the existing shows view for Audiobookshelf podcast servers. Populate it from Audiobookshelf's `GET /api/libraries/<ID>/recent-episodes` endpoint and reuse the existing episode presentation and actions.

## Boundaries & Constraints

**Always:** Scope requests to the selected podcast library and authenticated provider; preserve stable show and episode identities, server isolation, existing playback and basket actions, and localized labels. Treat the endpoint's result as newest unfinished episodes ordered by publish time. Keep pagination bounded and preserve its server order.

**Ask First:** Any change to podcast completion semantics or introduction of a new cross-provider browse mode.

**Never:** Fetch all shows to synthesize recents, expose credentials to the UI, or alter audiobook and music browsing.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Recent page | Podcast server with unfinished episodes | Recent Episodes tab shows episode cards/rows in server order, with play and basket actions | N/A |
| More results | Server reports more than one page | Load more appends without duplicates | Keep prior rows if later page fails |
| Empty | No unfinished episodes | Localized empty state in the tab | N/A |
| Unavailable | Auth, permission, stale library, or transport error | Existing podcast error treatment | No false empty state |
| Switch context | Change tab, server, or library while request runs | Ignore stale response | N/A |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/providers/audiobookshelf.rs` — Audiobookshelf HTTP client and podcast DTO mapping.
- `hifimule-daemon/src/providers/mod.rs` — provider-neutral podcast capability contract.
- `hifimule-daemon/src/rpc.rs` — podcast browse RPC routing and server resolution.
- `hifimule-ui/src/rpc.ts` — typed RPC client and podcast episode model.
- `hifimule-ui/src/library.ts` — podcast view, episode rows, state, loading, and actions.
- `hifimule-ui/src` locale resources — labels and empty/error text.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/mod.rs` and `hifimule-daemon/src/providers/audiobookshelf.rs` — add a bounded recent-episode page capability, map expanded response episodes to existing episode identities, and validate pagination and scope.
- [x] `hifimule-daemon/src/rpc.rs` and `hifimule-ui/src/rpc.ts` — expose typed recent-episode pages through the established browse RPC path.
- [x] `hifimule-ui/src/library.ts` and locale resources — add Shows and Recent Episodes tabs, loading/empty/error states, pagination, and stale-request protection; reuse episode rows.
- [x] Existing provider/RPC/UI test locations — cover mapping, paging, empty/error, and tab switching behavior.

**Acceptance Criteria:**
- Given an Audiobookshelf podcast library, when the user opens Recent Episodes, then it displays the endpoint's unfinished episodes in publish order.
- Given an episode in that tab, when the user plays or selects it for sync, then the existing episode action targets the correct show and podcast server.
- Given another server or tab is selected before a request completes, when that response arrives, then it does not replace the current content.

## Spec Change Log

## Verification

**Commands:**
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --target x86_64-pc-windows-msvc recent_podcast_episodes_` — provider tests pass using the verified FFmpeg runtime.
- `rtk node scripts/build-daemon.mjs test -p hifimule-daemon --target x86_64-pc-windows-msvc podcast_rpc_returns_typed_show_and_episode_fields` — RPC test passes.
- `rtk node --test tests/podcastRecents.test.mjs` in `hifimule-ui` — paging and stale-request tests pass.
- TypeScript check and Vite build in `hifimule-ui` — successful.

## Suggested Review Order

**Podcast library experience**

- See how Shows and Recent Episodes stay switchable through loading and errors.
  [library.ts:1803](../../hifimule-ui/src/library.ts#L1803)

- Follow request paging, stale-response checks, and retry behavior.
  [library.ts:1873](../../hifimule-ui/src/library.ts#L1873)

**Provider boundary**

- Review authenticated recent-episode retrieval and show identity resolution.
  [audiobookshelf.rs:2406](../../hifimule-daemon/src/providers/audiobookshelf.rs#L2406)

- Check scoped RPC validation and result shape.
  [rpc.rs:2076](../../hifimule-daemon/src/rpc.rs#L2076)

**Supporting checks**

- Inspect deduplication and server-page offset handling.
  [podcastRecents.ts:3](../../hifimule-ui/src/podcastRecents.ts#L3)

- Check provider identity and removed-show tests.
  [audiobookshelf.rs:5070](../../hifimule-daemon/src/providers/audiobookshelf.rs#L5070)

- Check UI page and stale-response tests.
  [podcastRecents.test.mjs:6](../../hifimule-ui/tests/podcastRecents.test.mjs#L6)
