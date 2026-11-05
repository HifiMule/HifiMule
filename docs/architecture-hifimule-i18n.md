# HifiMule i18n — Architecture

**Last Updated:** 2026-11-05 | **Scan depth:** Deep | **Workspace version:** 0.21.0

## Overview

`hifimule-i18n` is a shared Rust crate that embeds a JSON translation catalog at compile time and provides lookup functions. Both the daemon (Rust) and the UI (TypeScript via Vite alias) consume the same `catalog.json`.

## Catalog

- **File:** `catalog.json` (~335 KB, 3,902 lines)
- **Languages:** English (`en`, default), French (`fr`), Spanish (`es`), German (`de`)
- **Keys:** 1,061 flat dot-namespaced keys per language (parity enforced by tests)
- **Largest namespaces:** `playback` (316), `basket` (284), `library` (97), `profiles` (57), `device` (54), `playlist` (44), `lifecycle` (28), `login` (25), `repair` (25), `error` (19), `numeric` (16)

## API

| Function | Purpose |
|----------|---------|
| `system_language()` | Resolves language: `HIFIMULE_LANG` env → OS locale (Windows `GetUserDefaultLocaleName` / POSIX `LANG` etc.) → `"en"` |
| `normalize_language(lang)` | Lowercases, `_`→`-`, matches base subtag; unknown → `"en"` |
| `t(key)` | Translate using system language |
| `tf(key, replacements)` | Translate with `{placeholder}` interpolation |
| `translate(language, key)` | Translate in specific language with English fallback |
| `translate_with(language, key, replacements)` | Translate with interpolation and fallback |

## UI consumption

TypeScript imports `catalog.json` through a Vite alias (`src/i18n.ts`), using the same lookup and fallback logic.
