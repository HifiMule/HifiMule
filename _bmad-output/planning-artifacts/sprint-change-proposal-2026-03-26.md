# Sprint Change Proposal — Official Jellyfin API Alignment

**Date:** 2026-03-26
**Author:** Alexis (with SM agent)
**Status:** Approved (2026-03-26)

---

## 1. Issue Summary

**Problem Statement:** The daemon was calling Jellyfin using the legacy `/Users/{userId}/...` path prefix pattern for all data retrieval and playback reporting. These paths do not exist in `jellyfin-openapi-stable.json`. The official stable Jellyfin API places `userId` as a query parameter on top-level resource paths (e.g., `/Items?userId=`, `/UserViews?userId=`, `/UserPlayedItems/{itemId}?userId=`).

**Discovery:** Identified during Story 3.6 (Auto-Fill) implementation when `auto_fill.rs:76` was reviewed and found to use `{url}/Users/{userId}/Items`.

**Evidence:** Full audit of `api.rs` and `auto_fill.rs` against `docs/jellyfin-openapi-stable.json` revealed 4 endpoint pattern mismatches across 9 call sites. Two endpoints (`/System/Info`, `/Users/AuthenticateByName`) were already correct.

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact | Description |
|------|--------|-------------|
| Epic 3 | **Fix** | `get_views`, `get_items`, `get_item_details`, `get_items_by_ids`, `get_item_with_media_sources`, `get_child_items_with_sizes`, `search_audio_items` — all corrected |
| Epic 4 | **Fix** | `get_items_by_ids`, `get_item_with_media_sources`, `get_child_items_with_sizes` used by sync engine — corrected |
| Epic 5 | **Fix** | `report_item_played`, `search_audio_items` used by scrobbler — corrected |
| Epic 1, 2, 6 | None | Unaffected |

### Endpoint Audit

| Old (unofficial) | New (official spec) | Call site |
|---|---|---|
| `GET /Users/{id}/Views` | `GET /UserViews?userId={id}` | `get_views` |
| `GET /Users/{id}/Items{?params}` | `GET /Items?userId={id}&{params}` | `get_items` |
| `GET /Users/{id}/Items/{itemId}` | `GET /Items/{itemId}?userId={id}` | `get_item_details` |
| `GET /Users/{id}/Items?Ids=...` | `GET /Items?userId={id}&Ids=...` | `get_items_by_ids` |
| `GET /Users/{id}/Items/{itemId}?Fields=...` | `GET /Items/{itemId}?userId={id}&Fields=...` | `get_item_with_media_sources` |
| `GET /Users/{id}/Items?ParentId=...` | `GET /Items?userId={id}&ParentId=...` | `get_child_items_with_sizes` |
| `GET /Users/{id}/Items?SearchTerm=...` | `GET /Items?userId={id}&SearchTerm=...` | `search_audio_items` |
| `POST /Users/{id}/PlayedItems/{itemId}` | `POST /UserPlayedItems/{itemId}?userId={id}` | `report_item_played` |
| `GET /Users/{id}/Items?...` (auto_fill) | `GET /Items?userId={id}&...` | `auto_fill.rs:run_auto_fill` |

**Confirmed correct (no change):**
- `GET /System/Info`
- `POST /Users/AuthenticateByName`
- `GET /Items/{itemId}/Download`
- `GET /Items/{itemId}/Images/Primary`
- `SortBy=IsFavoriteOrLiked` (confirmed valid enum value in spec)

### Artifact Conflicts

| Artifact | Impact |
|----------|--------|
| **Code** | Fixed — 0 errors, compiling |
| **Architecture doc** | API patterns section needs update to reflect official paths |
| **Story 3.1 tech notes** | Endpoint references need updating |
| **Story 3.6 tech notes** | Endpoint reference needs updating |
| **Story 5.1 tech notes** | Endpoint reference needs updating |

---

## 3. Recommended Approach

**Direct Adjustment** — in-place code corrections. No scope change, no new stories, no epic restructuring. All code changes already applied and verified compiling.

**Effort:** Low — code done; artifact updates remaining
**Risk:** Low — no architectural changes, no IPC changes, same data model
**Timeline Impact:** None

---

## 4. Detailed Change Proposals (Applied)

### Code Changes (Done ✅)

**File:** `hifimule-daemon/src/api.rs`

All 8 call sites patched: `get_views`, `get_items`, `get_item_details`, `get_items_by_ids`, `get_item_with_media_sources`, `get_child_items_with_sizes`, `search_audio_items`, `report_item_played`.

**File:** `hifimule-daemon/src/auto_fill.rs`

`run_auto_fill` paginated endpoint patched.

### Artifact Updates (Remaining)

- Architecture doc: update API integration section to document correct endpoint patterns
- Stories 3.1, 3.6, 5.1: update tech notes referencing Jellyfin API paths

---

## 5. Implementation Handoff

### Change Scope: Minor

Code is done and compiling. Remaining work is documentation alignment only.

| Recipient | Responsibility |
|-----------|---------------|
| **Architect** | Update architecture doc — API section to reflect `/Items?userId=`, `/UserViews?userId=`, `/UserPlayedItems/{id}?userId=` patterns |
| **SM/Dev** | Update tech notes in Stories 3.1, 3.6, 5.1 to reference corrected endpoint paths |

### Success Criteria

- [x] All 9 unofficial endpoint call sites corrected in code
- [x] Build compiles cleanly (0 errors)
- [x] Architecture doc confirmed clean (no unofficial endpoint references found)
- [x] Implementation artifact tech notes updated: 3-3, 3-5, 3-6, 4-2, 5-1, tech-spec-fix-artist, tech-spec-server-sorted-autofill
