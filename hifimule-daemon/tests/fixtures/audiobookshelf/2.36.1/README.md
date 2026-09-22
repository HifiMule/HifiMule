# Audiobookshelf v2.36.1 contract fixtures

This fixture set is synthetic and redacted at authoring time. It records only field shape, typed semantics, status classes, and fixture-local aliases. It is not a captured server transcript.

`manifest.json` is schema version 1. New observations add a new case or field; they never rewrite an observed case as a pass. `verified` means observed against the controlled v2.36.1 deployment on 2026-09-21. `ambiguous` and `untested` are not implementation permission.

All real IDs, endpoint origins, titles, paths, credentials, tokens, authenticated URLs, request-header values, and error bodies are excluded. The contract document is the authority for endpoint templates and source evidence.

2026-09-22 correction: the synthetic book fixtures previously used `audioFiles[].id` and string chapter IDs. Live book-detail parsing exposed the audio-file mismatch; the Audiobookshelf API documents `audioFiles[].ino` as a string and chapter IDs as numbers. The fixtures now use those wire field types. Their fixture-local values remain synthetic.
