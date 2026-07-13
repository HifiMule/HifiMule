---
name: release
description: 'Prepare a HifiMule release: bump the version everywhere and draft both changelog artifacts in the house style, then stop before any git operation. Use when the user says "prepare a release", "cut a release", "/release <version>", or wants to bump the version and write release notes.'
---

# Release Prep

## Purpose

Do the local, pre-tag half of a HifiMule release in one step: bump the version in every place it
must stay in sync, then draft the two changelog artifacts in the established house style. **Stop
there** — the user reviews and edits the prose, then commits, tags, and pushes manually. Pushing a
`v*` tag is what triggers the CI release build (`.github/workflows/release.yml`), so this skill
never touches git.

## Inputs

- **Target version** (required): `X.Y.Z`, e.g. `0.13.0`. If the user invoked `/release <version>`,
  use that. If omitted, ask for it before doing anything.

## Steps

Run these in order. If a step fails, stop and report — do not continue to later steps.

### 1. Sanity-check the working tree

- `git status --porcelain`. If there are uncommitted changes unrelated to a release, warn the user
  and confirm before proceeding (the bump will mix into their working tree).

### 2. Determine the previous tag

- `git tag --sort=-v:refname | head -1` → this is `<prev-tag>` (e.g. `v0.12.0`). Used to scope the
  changelog to work since the last release.

### 3. Bump the version

- Run `node scripts/bump-version.mjs <version>`.
- This edits `Cargo.toml`, `hifimule-ui/src-tauri/tauri.conf.json`, and the `hifimule-*` entries in
  `Cargo.lock`. It validates semver and refuses to go backwards, and it performs no git operations.
- If it exits non-zero, surface its message and stop.

### 4. Gather source material for the changelog

- `git log <prev-tag>..HEAD --pretty=format:'%s%n%b' --no-merges` — the commit subjects/bodies.
- `git diff <prev-tag>..HEAD --stat` — the shape and scope of the change (which crates/areas moved).
- Read the two most recent entries at the top of `changelogs/CHANGELOG.md` and the latest
  `changelogs/CHANGELOG-*.md` to lock onto the house voice before writing.
- Use today's date (`YYYY-MM-DD`) as the release date.

### 5. Write `changelogs/CHANGELOG-<version>.md` (detailed)

Create a new file following this exact template (from `CHANGELOG-0.12.0.md`):

```markdown
# HifiMule <version>

Release date: <YYYY-MM-DD>

## Highlights

- **<Theme>**: <one-sentence user-facing summary>.
- ... (3–5 bullets, the headline changes only)

---

## Added

<subsections with ### headings when there are distinct features; bullet lists otherwise>

---

## Changed

- ...

---

## Fixed

- ...

---

## Internal

- ...
```

Rules:
- Omit any section (`Added` / `Changed` / `Fixed` / `Internal`) that has nothing real to report —
  do not emit empty headings.
- Group multi-part features under `### Subheadings` inside `Added`, like the 0.12.0 file does.
- Bullets are concrete and specific (name the feature, the file/area, the fixed symptom). Derive
  them from the commits and diff, not from imagination — if a commit is unclear, inspect the diff
  rather than guessing.
- `Internal` covers refactors, schema/test/i18n/build changes that users don't see.

### 6. Prepend a prose entry to `changelogs/CHANGELOG.md`

Insert a new block immediately under the top `# Changelog` header (above the current newest
`## v...` entry). Format:

```markdown
## v<version> - <YYYY-MM-DD>

<2–3 paragraphs of user-facing prose.>
```

House style for the prose (study the existing entries and match it):
- Present tense, second person ("you can…"), addressed to end users — not developers.
- **No bullet lists** in this file — flowing paragraphs only. Bold the occasional key feature name.
- Paragraph 1: the headline feature and what it lets the user do. Paragraph 2: secondary
  improvements / behavior changes. Paragraph 3 (optional): notable fixes.
- Describe user-visible value, not implementation. Keep `Internal`-only changes out of this file.

### 7. Stop and report

Do **not** run `git add`, `git commit`, `git tag`, or `git push`. Print a summary:

- Version bumped: `<prev> -> <version>` and the files the script changed.
- Changelog files written: `changelogs/CHANGELOG-<version>.md` (new) and the prepended block in
  `changelogs/CHANGELOG.md`.
- The exact manual follow-up for the user to run after reviewing/editing the prose:

  ```
  git add -A
  git commit -m "Release <version>"
  git tag v<version>
  git push && git push --tags
  ```

- Remind them that pushing the `v<version>` tag triggers the CI build, which produces a **draft**
  GitHub release to publish once the smoke test is green.

## Constraints

- Never perform git write operations (commit/tag/push) — the user does those after review.
- Never touch `hifimule-ui/package.json` (its version is intentionally decoupled at `0.1.0`).
- Do not invent changelog content; every entry must trace to a commit or a diff since `<prev-tag>`.
- If the target version already has a `changelogs/CHANGELOG-<version>.md` or a matching `## v` block,
  stop and ask — do not silently overwrite.
