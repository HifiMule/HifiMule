import english from 'virtual:hifimule-locale/en';

type Language = 'en' | 'fr' | 'es' | 'de';
type CatalogKey = string;

const DEFAULT_LANGUAGE: Language = 'en';

/** Keep supported locales aligned with the loaders and the Vite catalog plugin. */
const SUPPORTED_LANGUAGES: readonly Language[] = ['en', 'fr', 'es', 'de'];

const translations: Partial<Record<Language, Record<string, string>>> = { en: english };
const loaders = {
    fr: () => import('virtual:hifimule-locale/fr'),
    es: () => import('virtual:hifimule-locale/es'),
    de: () => import('virtual:hifimule-locale/de'),
};
const pending = new Map<Language, Promise<void>>();
let languageRevision = 0;

async function loadLanguage(language: Language): Promise<void> {
    if (translations[language] || language === 'en') return;
    let request = pending.get(language);
    if (!request) {
        request = loaders[language]().then(module => {
            translations[language] = module.default;
        }).finally(() => pending.delete(language));
        pending.set(language, request);
    }
    await request;
}

/** Await before rendering either webview. A missing local chunk falls back to English. */
export async function initializeI18n(): Promise<void> {
    try {
        await loadLanguage(currentLanguage());
    } catch (error) {
        console.warn('Could not load UI language; using English.', error);
    }
    applyDocumentLanguage();
}

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

export async function setLanguage(language: string): Promise<void> {
    const revision = ++languageRevision;
    const next = normalizeLanguage(language);
    await loadLanguage(next);
    if (revision !== languageRevision) return;
    localStorage.setItem('hifimule.language', next);
    applyDocumentLanguage();
}

/** Syncs the document's `lang` attribute to the active UI language so screen
 * readers announce content in the right language and the browser hyphenates
 * correctly (WCAG 3.1.1). The static `lang="en"` in index.html is only correct
 * for English users; this keeps it honest for everyone else. */
export function applyDocumentLanguage(): void {
    if (typeof document !== 'undefined') {
        const language = currentLanguage();
        document.documentElement.lang = translations[language] ? language : DEFAULT_LANGUAGE;
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
