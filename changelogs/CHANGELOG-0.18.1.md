# HifiMule 0.18.1

Release date: 2026-10-14

## Highlights

- **Audiobookshelf 2.37.1 playback**: Restore direct audiobook and podcast playback on newer 2.x servers instead of requiring exactly version 2.36.1.
- **Servers beneath a URL path**: Keep your configured Audiobookshelf path, such as `/audiobookshelf/`, when opening book parts and podcast episodes.
- **Scoped media validation**: Check that each direct media path matches the selected item and audio file before requesting audio.

---

## Changed

- Admit Audiobookshelf direct-play sessions across the 2.x version line, retaining item identity, playback method, supported format, and authenticated range checks. Controlled server observations cover 2.36.1 and 2.37.1; future minor and patch versions use the same compatibility policy.

---

## Fixed

- Stop rejecting audiobook and podcast direct-play sessions solely because the server reports 2.37.1 rather than 2.36.1.
- Resolve validated item-file routes beneath the configured deployment path when Audiobookshelf returns a route without that prefix. Accept an already-prefixed route when it matches the same item and file.
- Tighten audiobook media URL validation to the exact selected audio file, rejecting unrelated routes and paths containing traversal, query parameters, or fragments before media transport.

---

## Internal

- Add offline regression coverage for book and podcast version admission, both media URL forms under a deployment prefix, malformed or unsupported versions, unsafe paths, and session cleanup after rejection.
- Add an opt-in live provider smoke test for book and podcast discovery, direct-play admission, authenticated range validation, deployment-path preservation, and session retirement without listening-progress writes.
- Update the Audiobookshelf integration contract and implementation map with controlled 2.37.1 session and transport observations and the 2.x compatibility policy.
