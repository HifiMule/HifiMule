# Sprint Change Proposal — Selection-as-Playlist & List Curation Views

**Author:** Alexis
**Date:** 2026-06-05
**Workflow:** Correct Course (Change Navigation)
**Trigger:** GitHub Discussion [#6 — "Playlist Management → proper playlist editing"](https://github.com/HifiMule/HifiMule/discussions/6) by *plaquette*
**Change scope classification:** Moderate-to-Major (architectural trait amendment + new epic)

---

## Section 1 — Issue Summary

### Problem statement
Today HifiMule's device selection (the "basket") is **ephemeral and local**. Users build a selection, sync it to the device, and the selection is not durably stored anywhere reusable. Discussion #6 requests the ability to **persist a curated selection as a playlist on the media server**, to **curate that selection through an iTunes-style dual-panel view** (artists on the left, their albums on the right, filtered to the playlist's contents, with the ability to remove an entire artist or specific albums), and to **browse Artist/Album pages as lists/tables** rather than paginated album-art grids.

### Reframed objective (confirmed with user, 2026-06-05)
The core objective is to **use server playlists as the persistence layer for a device selection**. The dual-panel curation view and the list/table view paradigm are the UX that makes building and editing that selection efficient.

### Context of discovery
Raised externally by a community user (plaquette) who notes the feature is absent from Navidrome and comparable clients, referencing iTunes' interface as the model. This is a net-new capability landing mid-Phase-4 (implementation), with Epics 1–10 substantially complete. The maintainer (akartmann/Alexis) confirmed in-thread that HifiMule was originally conceived with iTunes in mind, so this direction is aligned with the product's founding intent.

### Discussion thread refinements (plaquette, Jun 5 2026)
The follow-up reply confirmed both an alternate selection view **and** playlist editing are in scope, and added these details:
- **Right-click context menus** to "send artist/album to a playlist" as the primary curation gesture.
- **Playlist statistics** — show total duration and storage size of a playlist.
- **Table views must handle thousands of items without pagination** — implies virtualized rendering, not just a flat list.
- **Bidirectional syncing with the server** — scoped (decision below) to **read-fresh + write-back**, not full concurrent-conflict resolution.
- **Auto-Fill by complete albums** instead of individual songs — handled as a **separate change proposal** (see Section 6).

### Evidence
- Basket is sync-time/local only; no persistence of selections — Stories 3.2 (basket), 4.7 (playlist M3U generation, read-only consumption of server playlists).
- `MediaProvider` trait is **read-only for playlists** — `list_playlists()` / `get_playlist()` only; no create/update/delete. The sole write-back operation is `scrobble()`. ([architecture.md:88](architecture.md))
- `Capabilities` has no playlist-write flag. ([architecture.md:422](architecture.md))
- Browse modes render as **paginated album-art grids** with breadcrumbs + A–Z quick-nav, not lists/tables. ([epics.md:430](epics.md))

---

## Section 2 — Impact Analysis

### 2.1 Epic Impact

| Epic | Status | Impact |
|------|--------|--------|
| **Epic 3** — Library Browser & Basket | done | Basket gains a "Save selection as playlist" action and a curation entry point. Additive; no rework of existing stories. |
| **Epic 8** — Provider trait & adapters | done | **Architectural amendment.** `MediaProvider` trait + both adapters (Jellyfin, Subsonic) gain playlist-write methods; `Capabilities` gains a write flag. |
| **Epic 9** — Browse modes & navigation UI | done | List/table view paradigm extends Artist & Album pages (currently paginated grids). Scoped as an Epic 9 extension story. |
| **Epic 4** — Sync engine (M3U gen) | done | No change required. Synergy: a saved server playlist flows through the existing 4.7 `.m3u` path for free. |

**Conclusion:** No existing epic can cleanly absorb the write-back capability + new curation UI. Introduce **new Epic 11 — Selection-as-Playlist & Curation**, plus one **Epic 9 extension** story for list views.

### 2.2 Story Impact
- **New stories:** Epic 11 (6 stories) + Epic 9 extension (1 story) — see Section 4.
- **No rollback** of completed stories required.
- **No invalidation** of future/planned work.

### 2.3 Artifact Conflicts

| Artifact | Conflict / change needed |
|----------|--------------------------|
| **PRD** | FR9 is read-only ("select playlists for sync"). Add FR37 (create/update server playlist from selection), FR38 (dual-panel curation + right-click "send to playlist" + playlist stats), FR39 (list/table browse views). NFR security note on write-scope; NFR performance note on virtualized large-list rendering. |
| **Architecture** | Trait amendment (write methods), `Capabilities.supports_playlist_write`, new RPCs `playlist.create/update/delete`, two adapter implementations, selection→tracks resolution. |
| **UX Spec** | New dual-panel curation component; list/table view added to Component Strategy (§5 currently grid-only). |

### 2.4 Technical Impact
- **New external failure surface:** writing to the user's server (auth scope, partial-failure handling, idempotency, conflict when playlist edited server-side concurrently).
- **Auth scope:** Jellyfin token and Subsonic password already carry write scope — no new credential model. Data privacy NFR remains satisfied (writes go only to the user's own server; zero third-party data).
- **Resolution logic:** basket entities (albums, artists, genres, individual tracks) must resolve to a concrete ordered track list at save time. **Auto-Fill virtual slot is EXCLUDED** (decision below).

### ⚠️ Resolved design decision — Auto-Fill
The Auto-Fill virtual slot resolves to tracks at *sync time*, not basket-build time ([prd.md:51](prd.md)). A saved playlist needs concrete tracks at *save time*. **Decision (user, 2026-06-05): EXCLUDE Auto-Fill from saved playlists.** Only manual selections (albums / artists / genres / individual tracks) are written to the server playlist; Auto-Fill remains a sync-time-only concept. The "Save as playlist" action surfaces a notice when an Auto-Fill slot is present in the basket so the user understands it won't be included.

---

## Section 3 — Recommended Approach

**Selected path: Option 1 — Direct Adjustment** (new Epic 11 + Epic 9 extension story).

**Rationale**
- **Strategically on-plan:** aligns with the PRD's own Phase 3 "Smart Playlists" roadmap ([prd.md:139](prd.md)) — an expansion, not a pivot.
- **Builds on existing patterns:** provider abstraction, RPC layer, basket, and M3U sync path are all reusable. Only the *direction* of the playlist data flow is new.
- **No rollback / no morale cost:** purely additive to completed, stable work.
- **Effort: Medium-High** — write APIs across two providers + two UI surfaces.
- **Risk: Medium** — new write-back failure surface; mitigated by capability-gating, single-provider rollout order (Jellyfin first), and excluding Auto-Fill to avoid stale-snapshot complexity.

**Alternatives considered**
- *Option 2 (Rollback):* N/A — nothing to revert; rejected.
- *Option 3 (MVP review):* N/A — MVP already shipped; this is post-MVP growth. Rejected.

---

## Section 4 — Detailed Change Proposals

### 4.1 PRD changes

**FR9 — clarify (read-only scope retained):**
> **OLD:** FR9: Users can select specific playlists or entities for synchronization.
> **NEW:** FR9: Users can select specific server playlists or entities (artists, albums, genres, tracks) for synchronization (read path). *Persisting a selection back to the server as a playlist is covered by FR37.*

**FR37 — NEW (write-back, read-fresh):**
> The system can persist the current device selection as a media-server playlist — creating a new playlist or updating an existing HifiMule-managed playlist. The system always reads the current server playlist state before editing (read-fresh) and writes the resulting track set back (write-back); it does not perform concurrent-conflict merge resolution. Basket entities (albums, artists, genres, individual tracks) are resolved to a concrete ordered track list at save time. The Auto-Fill virtual slot is excluded from the saved playlist; when present, the user is notified it will not be included. Supported on Jellyfin (`POST /Playlists`, `POST/DELETE /Playlists/{id}/Items`) and Subsonic/OpenSubsonic (`createPlaylist`, `updatePlaylist`, `deletePlaylist`), gated by a provider capability flag.

**FR38 — NEW (curation view):**
> The system provides a dual-panel playlist curation view: artists in the playlist on the left, that artist's albums on the right, both filtered to the playlist's current contents. The user can remove an entire artist from the playlist, or remove specific albums of an artist. A right-click (context-menu) gesture lets the user send an artist or album to a playlist directly from browse views. The view displays playlist statistics — total track count, total duration, and total storage size. Edits update the server playlist (FR37).

**FR39 — NEW (list/table browse):**
> The system can present Artist and Album browse pages as browsable list/table views (in addition to or replacing paginated album-art grids), enabling rapid scanning and multi-item selection for basket and playlist curation. The list/table view must render large libraries (thousands of items) smoothly via virtualized rendering rather than pagination.

**NFR — Security & Privacy (append):**
> Playlist write operations target only the user's configured media server using existing stored credentials (Jellyfin token / Subsonic per-request token). No new credential scope is introduced and zero data is transmitted to third-party servers.

**NFR — Performance (append):**
> List/table browse views must use virtualized (windowed) rendering to remain responsive with libraries of thousands of items, avoiding pagination while keeping memory and scroll performance within the app's existing UI responsiveness targets.

### 4.2 Architecture changes

**`MediaProvider` trait — append write methods:**
```rust
// Playlist write capability (capability-gated; callers must check capabilities().supports_playlist_write)
async fn create_playlist(&self, name: &str, track_ids: &[String]) -> Result<Playlist, ProviderError>;
async fn update_playlist(&self, playlist_id: &str, track_ids: &[String]) -> Result<(), ProviderError>; // full replace of track set
async fn delete_playlist(&self, playlist_id: &str) -> Result<(), ProviderError>;
```

> **Design refinement (post-proposal):** `update_playlist` was replaced by separate `add_to_playlist(playlist_id, track_ids)` and `remove_from_playlist(playlist_id, track_ids)` methods in the final architecture and story specs. The daemon RPC layer computes the diff between the current server playlist state and the desired state, then calls add/remove as needed. The RPCs are also named `playlist.addTracks` and `playlist.removeTracks` (not `playlist.update`). This document's Section 4.2 reflects the earlier draft; see `architecture.md` (Epic 11 section) and `epics.md` (Stories 11.1–11.4) for the final design.

**`Capabilities` — add flag:**
```rust
pub struct Capabilities {
    pub open_subsonic: bool,
    pub supports_changes_since: bool,
    pub supports_server_transcoding: bool,
    pub supports_playlist_write: bool,   // NEW — Jellyfin: true; Subsonic/OpenSubsonic: true; degrade gracefully if write probe fails
}
```

**New daemon RPCs:**
- `playlist.create(params: { name: string, basketSnapshot: ... }) → { playlistId: string }`
- `playlist.update(params: { playlistId: string, basketSnapshot: ... }) → { ok: true }`
- `playlist.delete(params: { playlistId: string }) → { ok: true }`
- Selection→tracks resolution lives in the daemon (reuses artist/genre/album expansion already used by the sync engine); Auto-Fill slot is skipped.

**Adapter implementations:**
- **JellyfinProvider:** `POST /Playlists` (create), `POST /Playlists/{id}/Items` + `DELETE /Playlists/{id}/Items` (update as add/remove diff), `DELETE /Playlists/{id}` (delete).
- **SubsonicProvider:** `createPlaylist`, `updatePlaylist` (supports add/remove by index), `deletePlaylist`.

**UI must hide/disable the "Save as playlist" affordance when `capabilities().supports_playlist_write == false`** (consistent with existing capability-driven browse-mode hiding, [architecture.md:337](architecture.md)).

### 4.3 UX Spec changes

**§5.2 Custom Components — add:**
- **Playlist Curation View (dual-panel):** Left panel lists artists present in the playlist; selecting an artist shows that artist's albums (filtered to playlist contents) in the right panel. Remove-artist and remove-album affordances. Displays a playlist statistics header (track count · total duration · total size). Mirrors the iTunes reference from discussion #6.
- **Context Menu (right-click):** A right-click menu on artists/albums in browse and curation views offering "Send to playlist…" (create new or pick existing managed playlist) and remove actions.

**§5 Component Strategy — add:**
- **List/Table Browse View:** A *virtualized* list/table rendering mode for Artist and Album pages as an alternative to the paginated `<sl-card>` grid, optimized for scanning and multi-select across thousands of items. Honors the existing A–Z quick-nav and breadcrumb patterns.

**Basket (§5.2) — add:**
- **"Save selection as playlist" action** in the basket header: prompts for a playlist name (or pick an existing HifiMule-managed playlist to update). Shows an inline notice when an Auto-Fill slot is present ("Auto-Fill tracks are resolved at sync time and won't be saved to this playlist").

### 4.4 Epic & Story changes

**NEW — Epic 11: Selection-as-Playlist & Curation**
1. **11.1** — `MediaProvider` playlist-write trait amendment + `Capabilities.supports_playlist_write` + provider-neutral resolution contract.
2. **11.2** — JellyfinProvider playlist write adapter (create/update/delete) + tests.
3. **11.3** — SubsonicProvider/OpenSubsonic playlist write adapter (create/update/delete) + tests.
4. **11.4** — Daemon RPCs `playlist.create/update/delete` + selection→tracks resolution (excludes Auto-Fill).
5. **11.5** — Basket "Save selection as playlist" UI (create new / update managed) + right-click "send to playlist" from browse views + Auto-Fill exclusion notice + capability gating.
6. **11.6** — Dual-panel playlist curation view (remove artist / remove albums → update server playlist) + playlist statistics header (count · duration · size).

**Epic 9 extension (new story):**
- **9.7** — Virtualized list/table view for Artist & Album browse pages handling thousands of items (toggle alongside grid; preserves quick-nav/breadcrumbs/add-to-basket).

**Suggested sequencing:** 11.1 → (11.2, 11.3 parallel) → 11.4 → 11.5 → 11.6; 9.7 independent (can run in parallel). Jellyfin (11.2) first to de-risk the write path before Subsonic.

---

## Section 5 — Implementation Handoff

**Scope classification: Moderate-to-Major.** The trait amendment is architectural, warranting a light Architect pass before story creation; the remainder follows the normal PO/Dev story cycle.

| Step | Owner | Deliverable |
|------|-------|-------------|
| 1. Architecture touch-up | Architect (`bmad-create-architecture`, targeted) | Trait + Capabilities + RPC contract section updated in `architecture.md` |
| 2. Epic/story authoring | PM/PO (`bmad-create-epics-and-stories` or `bmad-create-story`) | Epic 11 (11.1–11.6) + Story 9.7 added to `epics.md` |
| 3. Readiness check | (`bmad-check-implementation-readiness`) | PRD/UX/Arch/Epics alignment confirmed |
| 4. Implementation | Dev (`bmad-create-story` → `bmad-dev-story` → `bmad-code-review`) | Per-story cycle |
| 5. sprint-status.yaml | (this workflow / PO) | Add `epic-11: backlog` + stories; add `9-7` under epic-9 |

**Success criteria**
- A user can save the current basket selection as a new server playlist on both Jellyfin and Subsonic, and update it later.
- The dual-panel curation view can remove an artist or specific albums, persisting to the server playlist.
- Artist/Album pages offer a list/table view.
- Auto-Fill is correctly excluded with a clear user notice.
- Saved playlists sync to the device as `.m3u` via the existing 4.7 path with no regression.

---

## Section 6 — Captured for Separate Handling

These items surfaced in discussion #6 but are intentionally **out of scope** for this proposal to keep it focused. They are recorded here so they are not lost:

1. **Auto-Fill redesign (selectable algorithms)** — plaquette's request for album-level Auto-Fill is one input into a broader planned redesign of the Auto-Fill feature: letting the user **choose the fill algorithm** (e.g. complete-albums, individual-tracks by favorites/play-count/recency, and future strategies) rather than the single fixed priority order in FR29 ([prd.md:51](prd.md)). This is a deliberate, separate initiative the maintainer will scope globally — **not** addressed here. Recorded so the album-level request is folded into that future redesign rather than handled piecemeal.
2. **Full conflict-resolving bidirectional sync** — true two-way merge of concurrent edits (server-side edits vs HifiMule edits vs device-side `.m3u` edits) is deferred. This proposal implements **read-fresh + write-back** only (FR37). A future epic could add conflict detection/merge if demand emerges.

---

## Approval

- [x] **Approved for implementation** — Alexis, 2026-06-05
- [ ] Revise (capture feedback below)

_Notes:_ Auto-Fill album-level request deferred into a future global Auto-Fill redesign (selectable algorithms) — see Section 6.1. Bidirectional scope fixed at read-fresh + write-back. Auto-Fill excluded from saved playlists.
