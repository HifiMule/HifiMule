---
title: 'Audiobookshelf folder artwork through the shared preview pipeline'
type: 'bugfix'
created: '2026-10-22'
status: 'done'
baseline_commit: 'a54dc50cd4519c6bb85d9539f33eba54d41112ff'
context:
  - '{project-root}/docs/audiobookshelf-integration-contract.md'
---

<frozen-after-approval reason="human-owned intent â€” do not modify unless human renegotiates">

## Intent

**Problem:** Synced Audiobookshelf audiobook and podcast folders receive no folder images because the shared artwork pipeline accepts only music. Its artist/album layout assumption also does not fit podcast show folders.

**Approach:** Extend the existing shared folder-preview pipeline to all Audiobookshelf content folders actually created by synchronization: audiobook author folders, audiobook title folders, and podcast show folders. Fetch available provider images and reuse the existing JPEG processing, portable publication, native folder icons, ownership, and cleanup functions.

## Boundaries & Constraints

**Always:** Honor the artwork switch, role roots, physical identities, cancellation, capacity/image limits, and unmanaged files. Use `MediaProvider` and daemon-only bearer authentication. Prefer author portraits; retain the existing book-cover fallback. Preserve music behavior, deterministic source selection, and inline artwork before track completion. Updates/full sync enable artwork; unchanged sync retains its current behavior.

**Ask First:** Changes to root-folder artwork policy, artwork defaults or size limits, unmanaged-file preservation, or unchanged-library backfill.

**Never:** Duplicate preview/icon code, add dependencies/settings, identify sources by title, expose authenticated URLs, edit audio tags, or mutate servers. Series/collections sync into ordinary author/title folders; introduce no new grouping folders.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|---------------|----------------------------|----------------|
| Audiobook | Configured root/Author/Book/part | Author image and book cover use shared previews | Missing images leave audio successful |
| Podcast | Configured root/Show/episode | Show cover uses shared previews; root gets no icon | Missing cover or removed show is nonfatal |
| Sparse author | No portrait, or no stable author identity | Retain existing provider book-cover fallback where resolvable; otherwise skip author image | Never identify authors by folder names |
| Scope boundary | Custom/nested roots, aliases, overlapping roots, or old retained paths | Each candidate must be strictly below its own media-role root; exclude managed roots themselves | Uncertain identities preserve ownership |
| Existing content | Owned JPEG/native records or unmanaged images | Reuse owned output and preserve user content | Existing safe publication and cleanup rules apply |
| Disabled/unchanged | Switch off, or normal sync with no audio update | No new artwork requests | Full sync can repair missing previews |

</frozen-after-approval>

## Code Map

- `hifimule-daemon/src/sync/artwork.rs` â€” music-only candidate/root gates, two-level targets, artist/album resolution; existing shared publisher and cleanup.
- `hifimule-daemon/src/sync.rs` â€” role-specific output paths; writer already calls artwork before track completion.
- `hifimule-daemon/src/providers/audiobookshelf.rs` â€” existing book/show fetching; missing author portraits; raw author IDs conflict with scoped `get_artist` IDs.
- `hifimule-daemon/src/rpc.rs` â€” `podcast_episode_to_desired` drops available cover metadata.
- `docs/audiobookshelf-implementation.md` â€” provider documentation.
- [Upstream API](https://api.audiobookshelf.org/#get-an-author-s-image) â€” optional author `imagePath` and authenticated `/api/authors/{id}/image` reads.

## Tasks & Acceptance

**Execution:**
- [x] `hifimule-daemon/src/providers/audiobookshelf.rs` â€” scope author IDs consistently; expose/fetch portraits through protected transport; validate library membership; retain absent-portrait book fallback. Test portrait/authentication, absent images, scope, IDs, and failures.
- [x] `hifimule-daemon/src/sync/artwork.rs` â€” share role-aware targets between retained and transferred items: audiobook author/book and podcast show. Resolve podcast covers through podcast APIs even without plan hints. Check each candidate against its physical role root; exclude managed roots. Reuse publication/cleanup.
- [x] `hifimule-daemon/src/rpc.rs` â€” carry podcast cover metadata into desired-item artwork; extend mapping tests.
- [x] `hifimule-daemon/src/sync/artwork.rs`, `hifimule-daemon/src/sync.rs` â€” test the matrix with mocked providers/temporary devices: three folder types, nested roots, aliases, cleanup, disabled/unchanged sync, missing images, and repeat-download avoidance.
- [x] `docs/audiobookshelf-implementation.md` â€” document folder types and update/full-sync timing.

**Acceptance Criteria:**
- Given enabled artwork and an audiobook/podcast transfer, when the writer reports completion, then available previews have passed through shared publication/native icons.
- Given mixed media roles, when syncing/removing content, then role roots, music previews, and ownership protections remain correct.
- Given an image/provider failure, when artwork runs, then audio remains successful and warnings expose no credentials.

## Spec Change Log

## Verification

**Commands:**
- `rtk npm run build:daemon -- test -p hifimule-daemon artwork -- --quiet` â€” shared artwork/writer regressions pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon audiobookshelf -- --quiet` â€” provider regressions pass.
- `rtk npm run build:daemon -- test -p hifimule-daemon podcast -- --quiet` â€” podcast regressions pass.
- `rtk git diff --check` â€” clean.

These are offline checks; live server/file-manager rendering remains unverified.

### Execution evidence â€” 2026-10-22

The new folder-target regression failed before implementation (`missing Books/Author`). Final focused checks passed:

- `artwork`: 27 passed, 0 failed.
- `audiobookshelf`: 92 passed, 0 failed, 1 ignored (opt-in live smoke test).
- `podcast`: 37 passed, 0 failed, 1 ignored.
- `rtk git diff --check`: clean.

`npm` was unavailable on the execution shell's PATH. The equivalent `scripts/build-daemon.mjs test -p hifimule-daemon <filter> -- --quiet` calls ran through the bundled Node executable, with its directory added to the child PATH. The repository's Windows audio-runtime verification and cargo wrapper ran successfully. Existing unrelated compiler/FFmpeg warnings remain. Logs are in ignored `target/abs-artwork-tests.log`, `target/abs-provider-tests.log`, and `target/abs-podcast-tests.log`.

Regression coverage includes real Audiobookshelf mock transport and temporary-device author/title/show publication, native icon artifacts, selected-library portrait membership, scoped author IDs, JPEG negotiation, bearer refresh, missing images and 403/404/500 failures, nested/overlapping physical roots and aliases, repeated-download avoidance, disabled/unchanged sync, owned cleanup, and the existing inline ordering/full-sync checks. A real Audiobookshelf audio transfer also asserts folder previews exist on successful completion. Live server and file-manager rendering were not tested.

### Independent review

Blind Hunter, Edge Case Hunter, and Acceptance Auditor reviewed the complete change independently. No actionable findings remained. Parent verification inspected final test logs, source diffs, the completed task checklist, and a clean whitespace check.

## Suggested Review Order

**Folder selection**

- Reuse the transfer hook for every media role.
  [artwork.rs:506](../../hifimule-daemon/src/sync/artwork.rs#L506)

- Select author/title or show folders and enforce physical role boundaries.
  [artwork.rs:52](../../hifimule-daemon/src/sync/artwork.rs#L52)

**Provider images**

- Resolve podcast images through show and episode APIs.
  [artwork.rs:276](../../hifimule-daemon/src/sync/artwork.rs#L276)

- Fetch scoped author portraits with bearer authentication and JPEG negotiation.
  [audiobookshelf.rs:3671](../../hifimule-daemon/src/providers/audiobookshelf.rs#L3671)

- Carry podcast cover hints into the existing sync plan.
  [rpc.rs:5212](../../hifimule-daemon/src/rpc.rs#L5212)

**Verification**

- Exercise shared publication, reuse, native outputs, and cleanup for Audiobookshelf folders.
  [artwork.rs:868](../../hifimule-daemon/src/sync/artwork.rs#L868)

- Check portrait scoping, fallback, refresh, and failures.
  [audiobookshelf.rs:4921](../../hifimule-daemon/src/providers/audiobookshelf.rs#L4921)

- Verify previews exist when real Audiobookshelf audio transfer completes.
  [sync.rs:7033](../../hifimule-daemon/src/sync.rs#L7033)

**Documentation**

- Document folder support and update/full-sync timing.
  [audiobookshelf-implementation.md:42](../../docs/audiobookshelf-implementation.md#L42)

## Follow-up — explicit image format and resize requests

The user reported `Invalid or oversized artwork` for book and podcast folders. Item cover requests omitted the format and dimensions, allowing Audiobookshelf's default WebP response, while the shared decoder supports PNG/JPEG. All book, podcast, and author image requests now specify `format=jpeg&width=500`; height is omitted to preserve aspect ratio. The existing shared preview conversion retains its final 500-pixel bound and safety limits.

The lifecycle regression was tightened to require these query parameters for both cover and portrait transport and failed before the fix. The complete artwork filter then passed 27 tests; the Audiobookshelf provider filter passed 92 tests with one opt-in live test ignored. The audio-transfer regression also asserts the image request parameters. Live retrieval of the user's affected images remains untested.
