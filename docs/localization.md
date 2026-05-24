# Localization Guide

HifiMule uses a shared translation catalog for both the Rust daemon and the Tauri UI.

## Files

- `hifimule-i18n/catalog.json` is the single source of truth for user-facing translations.
- `hifimule-i18n/src/lib.rs` loads the catalog for Rust daemon code.
- `hifimule-ui/src/i18n.ts` loads the same catalog for TypeScript UI code.
- `hifimule-ui/tsconfig.json` and `hifimule-ui/vite.config.ts` expose the shared catalog to the UI as `@hifimule/i18n-catalog`.

## Language Selection

Daemon language selection order:

1. `HIFIMULE_LANG`
2. OS user locale, when available
3. `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`
4. English fallback

UI language selection order:

1. `localStorage.getItem('hifimule.language')`
2. Browser `navigator.language`
3. English fallback

Both sides normalize language tags to the base language. For example, `fr-FR`, `fr_CA`, and `fr` all resolve to `fr`; `es-ES`, `es_MX`, and `es` all resolve to `es`.

Currently supported languages:

- `en` — English
- `fr` — French
- `es` — Spanish

## Add A New Language

1. Add a new top-level object to `hifimule-i18n/catalog.json`.

   Example for German:

   ```json
   {
     "de": {
       "app.name": "HifiMule",
       "server.default": "Server"
     }
   }
   ```

2. Copy every key from the English `en` catalog into the new language object.

   Do not omit keys. Missing keys fall back to English, but complete catalogs make review easier.

3. Preserve placeholders exactly.

   If English has `{count}`, `{size}`, `{message}`, `{name}`, `{profile}`, `{pct}`, `{completed}`, or `{total}`, the translated string must keep the same placeholder names.

   Good:

   ```json
   "basket.sync.file_counter": "{completed} de {total} archivos"
   ```

   Bad:

   ```json
   "basket.sync.file_counter": "{done} de {all} archivos"
   ```

4. Update language normalization in both runtimes.

   In `hifimule-i18n/src/lib.rs`, add the base language code to `normalize_language()`:

   ```rust
   match lower.split('-').next().unwrap_or(DEFAULT_LANGUAGE) {
       "fr" => "fr".to_string(),
       "es" => "es".to_string(),
       "de" => "de".to_string(),
       _ => DEFAULT_LANGUAGE.to_string(),
   }
   ```

   In `hifimule-ui/src/i18n.ts`, add the language to the `Language` type and `normalizeLanguage()`:

   ```ts
   type Language = 'en' | 'fr' | 'es' | 'de';
   return base === 'fr' || base === 'es' || base === 'de' ? base : DEFAULT_LANGUAGE;
   ```

   Also update `hifimule-ui/src/i18n-catalog.d.ts` to include the new code.

5. Test language selection.

   UI quick test in the browser console:

   ```js
   localStorage.setItem('hifimule.language', 'de')
   location.reload()
   ```

   Daemon quick test:

   ```powershell
   $env:HIFIMULE_LANG='de'
   rtk cargo run -p hifimule-daemon
   ```

6. Run verification.

   ```bash
   rtk cargo test -p hifimule-i18n
   rtk cargo check -p hifimule-daemon
   cd hifimule-ui
   rtk npm run build
   ```

## Add Or Change A String

1. Add the key to every language object in `hifimule-i18n/catalog.json`.
2. Use `hifimule_i18n::t("key")` or `hifimule_i18n::tf("key", &[("name", value)])` in Rust.
3. Use `t('key')` or `t('key', { name: value })` in TypeScript.
4. Avoid hardcoded user-facing strings in daemon notifications, tray labels, UI labels, buttons, and visible errors.

## Notes

- English is the fallback language.
- Accents and non-ASCII characters are allowed in translations.
- Keep protocol values untranslated. RPC method names, enum values such as `artists`, and manifest fields must remain stable.
- Prefer concise UI labels. Some controls are narrow, especially the basket sidebar and navigation mode bar.
