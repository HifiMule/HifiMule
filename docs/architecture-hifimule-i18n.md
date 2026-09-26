# HifiMule i18n — Architecture

**Last Updated:** 2026-09-27 | **Scan depth:** Deep | **Workspace version:** 0.15.0

## Purpose and data flow

`hifimule-i18n` is the Rust translation library. `hifimule-i18n/catalog.json` is the shared catalog: the Rust crate embeds it with `include_str!`, while the TypeScript UI imports the same file through `@hifimule/i18n-catalog`. The daemon depends on the crate directly; the UI uses its TypeScript adapter in `hifimule-ui/src/i18n.ts`.

## Language behavior

Both runtimes support English, French, Spanish, and German. Rust checks `HIFIMULE_LANG`, then system/POSIX locale, and falls back to English. The UI checks `hifimule.language` in local storage, then `navigator.language`, and falls back to English. Both normalize regional tags to a base language. Missing catalog keys fall back to English, then to the key itself. `t`, `tf`, `translate`, and `translate_with` expose lookup and placeholder interpolation in Rust; the UI exports `t` and `setLanguage` and updates the document language for accessibility.

## Development and tests

Add strings to every supported language in `catalog.json`, preserving placeholder names. Add any new language to normalization on both sides and to the UI catalog declaration. Run `rtk cargo test -p hifimule-i18n` and the UI build. The Rust tests cover fallback, regional tags, interpolation, and complete playback control keys across supported locales.

See the [Localization Guide](./localization.md) for the editing procedure.
