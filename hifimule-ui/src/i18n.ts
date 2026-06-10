import catalog from '@hifimule/i18n-catalog';

type Language = 'en' | 'fr' | 'es' | 'de';
type CatalogKey = string;

const DEFAULT_LANGUAGE: Language = 'en';

/** Languages the catalog ships translations for. Adding a language here (and to
 * the `Language` union) is all the wiring an already-translated locale needs —
 * the catalog is the source of truth. */
const SUPPORTED_LANGUAGES: readonly Language[] = ['en', 'fr', 'es', 'de'];

const translations: Record<string, Record<string, string> | undefined> = catalog;

function isSupported(language: string): language is Language {
    return (SUPPORTED_LANGUAGES as readonly string[]).includes(language);
}

function normalizeLanguage(language: string | null | undefined): Language {
    const base = language?.trim().toLowerCase().replace('_', '-').split('-')[0];
    return base && isSupported(base) ? base : DEFAULT_LANGUAGE;
}

export function currentLanguage(): Language {
    return normalizeLanguage(
        localStorage.getItem('hifimule.language')
        || navigator.language
        || DEFAULT_LANGUAGE.toString()
    );
}

export function setLanguage(language: string): void {
    localStorage.setItem('hifimule.language', normalizeLanguage(language));
    applyDocumentLanguage();
}

/** Syncs the document's `lang` attribute to the active UI language so screen
 * readers announce content in the right language and the browser hyphenates
 * correctly (WCAG 3.1.1). The static `lang="en"` in index.html is only correct
 * for English users; this keeps it honest for everyone else. */
export function applyDocumentLanguage(): void {
    if (typeof document !== 'undefined') {
        document.documentElement.lang = currentLanguage();
    }
}

export function t(key: CatalogKey, replacements: Record<string, string | number> = {}): string {
    const language = currentLanguage();
    // A locale object can be absent (catalog drift) — fall back through the
    // default language to the raw key rather than throwing on `undefined[key]`.
    const template =
        translations[language]?.[key]
        ?? translations[DEFAULT_LANGUAGE]?.[key]
        ?? key;
    return Object.entries(replacements).reduce(
        (text, [name, value]) => text.split(`{${name}}`).join(String(value)),
        template
    );
}

// Keep <html lang> correct from first paint, before any view renders.
applyDocumentLanguage();
