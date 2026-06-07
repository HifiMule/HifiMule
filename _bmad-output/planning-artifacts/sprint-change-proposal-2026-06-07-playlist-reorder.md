# Sprint Change Proposal: Playlist Track Reordering — Curation View

**Date:** 2026-06-07
**Trigger:** Epic 11 (Selection-as-Playlist & Curation) — post Story 11.8
**Proposed by:** Alexis
**Scope:** Moderate — backend provider/RPC + frontend; PO/DEV handoff

---

## 1. Issue Summary

### Problem Statement

The playlist curation view (`PlaylistCurationView.ts`) lets users add, remove, rename, and delete playlists and their tracks, but provides **no way to reorder tracks**. Users want to:

1. View the **complete** playlist as a single ordered track list (not only the tracks of one selected artist/album).
2. **Change the order** of tracks.
3. See each track's **absolute order number**, so reordering remains meaningful even when an artist or album filter is active.

### Discovery Context

Identified by Alexis during continued playlist-editor work on the `playlist-edit` branch, after Stories 11.1–11.8 were completed.

### Root Cause

Reordering was never in Epic 11's original scope. FR37/FR38 covered create, curate, add, remove, rename, and delete — sequence/ordering was not a requirement. A full-stack trace confirms reordering exists nowhere today:

| Layer | Current state |
|---|---|
| `MediaProvider` trait (`providers/mod.rs`) | create / add / remove / delete / rename only — **no reorder method** |
| Jellyfin adapter (`providers/jellyfin.rs`) | add appends, remove by entry-id — **Items/Move endpoint unused** |
| Subsonic adapter (`providers/subsonic.rs`) | add/remove by index — **no reorder** |
| RPC dispatch (`rpc.rs`) | `playlist.create/addItems/addTracks/removeTracks/delete/rename` — **no `playlist.reorder`** |
| `BrowseTrack` / `Song` model | has `trackNumber` (album track #) — **no playlist-position field** (providers do preserve server order on fetch) |

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact |
|---|---|
| Epic 11 — Selection-as-Playlist & Curation | Scope **extends** with 2 new stories (11.9 backend, 11.10 frontend). Remains `in-progress`. |
| All other epics | None. |

### Story Impact

| Story | Change |
|---|---|
| 11.9 (new, backlog) | `reorder_playlist` trait method + Jellyfin & Subsonic adapters + `playlist.reorder` RPC. |
| 11.10 (new, backlog) | Curation view: "All artists/All albums" complete list, `#N` order numbers, up/down reorder controls. |
| 11.1–11.8 | Unaffected. |

### Artifact Conflicts

| Artifact | Section | Change |
|---|---|---|
| `prd.md` | FR38 (touch-up), **FR40 (new)** | Order column + complete-list view; new reorder capability FR. |
| `epics.md` | FR Coverage Map; Stories 11.9, 11.10 | Add `FR40` mapping; add two stories. |
| `ux-design-specification.md` | §5.2 Playlist Curation View | Complete-list, order numbers, up/down reorder. |
| `sprint-status.yaml` | Epic 11 | Add 11-9 and 11-10 as `backlog`. |
| `hifimule-i18n/catalog.json` | en/fr/es | New keys (Story 11.10). |
| `PlaylistCurationView.ts` | — | Build target (Story 11.10). |

### Technical Impact

New provider contract (set-order semantics — one abstraction both providers satisfy):

```rust
async fn reorder_playlist(&self, playlist_id: &str, ordered_track_ids: &[String]) -> Result<(), ProviderError>
```

- **Subsonic/OpenSubsonic:** `createPlaylist?playlistId={id}&songId=…` in order — replaces contents in the given order (one native call).
- **Jellyfin:** selection-sort via `POST /Playlists/{id}/Items/{playlistItemId}/Move/{index}` (no removal; preserves entry identity; O(n) move calls — acceptable for DAP-sized playlists).
- **RPC:** `playlist.reorder({ playlistId, trackIds })`, gated by the existing `supports_playlist_write` capability — **no new capability flag**.

Frontend (decisions: up/down buttons; move within filtered subset):

- "All artists" / "All albums" entries → track panel shows the complete playlist in order.
- `#N` 1-based absolute position on every track row, shown under any filter.
- ↑/↓ icon-buttons per row (when `supports_playlist_write`): swap absolute positions with the previous/next **visible** track; optimistic local update → `playlist.reorder` with the full reordered id list; rollback + inline error on failure. ↑ disabled on first visible row, ↓ on last.

---

## 3. Recommended Approach

**Option 1 — Direct Adjustment ✅ Selected**

- **Effort:** Medium. **Risk:** Low–Medium (the Jellyfin Items/Move selection-sort is the only non-trivial piece).
- Add Stories 11.9 (backend) and 11.10 (frontend) to Epic 11. No existing code reverted; the change is purely additive.

Option 2 (Rollback) — **N/A**: nothing to revert. Option 3 (MVP review) — **N/A**: additive, MVP unaffected.

**Backend granularity:** one story (11.9) for trait + both adapters + RPC, per Alexis's decision (it is a single method).

---

## 4. Detailed Change Proposals

The concrete edits (FR40, FR38 touch-up, FR Coverage Map, Stories 11.9 & 11.10, UX §5.2, sprint-status entries) were reviewed and approved in Incremental mode and are applied to the artifacts. See:

- `prd.md` — FR40 (new) + FR38 addendum
- `epics.md` — FR Coverage Map `FR40`; Story 11.9; Story 11.10
- `ux-design-specification.md` — §5.2 Playlist Curation View addendum
- `sprint-status.yaml` — `11-9-…: backlog`, `11-10-…: backlog`

---

## 5. Implementation Handoff

**Scope Classification:** Moderate — backend contract + RPC + frontend across two stories.

**Sequencing:** 11.9 (backend) → 11.10 (frontend, depends on `playlist.reorder` RPC).

**Files in scope:**

| Story | Files |
|---|---|
| 11.9 | `providers/mod.rs`, `providers/jellyfin.rs`, `providers/subsonic.rs`, `rpc.rs` (+ tests) |
| 11.10 | `hifimule-ui/src/components/PlaylistCurationView.ts`, `hifimule-ui/src/rpc.ts`, `hifimule-i18n/catalog.json` |

**Success criteria:**
- `reorder_playlist` produces the exact requested order on both Jellyfin and Subsonic; `NotSupported` on incapable providers; daemon `playlist.reorder` capability-gated.
- Curation view shows "All artists/All albums", `#N` order numbers (under any filter), and working ↑/↓ that persist via `playlist.reorder` with optimistic update + rollback on error.
- `rtk cargo test` and `rtk tsc` pass with zero errors.

**Handoff:**
- **Product Owner / DEV:** add Stories 11.9–11.10 to the sprint (done in `sprint-status.yaml`); run `create-story` for each to produce context-filled story files.
- **Developer agent:** implement 11.9 then 11.10.

---

## 6. Approval

Proposals reviewed in Incremental mode; backend confirmed as a single story (11.9). Artifacts updated 2026-06-07.
