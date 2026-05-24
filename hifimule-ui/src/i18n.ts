import catalog from '@hifimule/i18n-catalog';

type Language = 'en' | 'fr' | 'es';
type CatalogKey = string;

const DEFAULT_LANGUAGE: Language = 'en';
const translations = catalog as Record<Language, Record<string, string>>;

function normalizeLanguage(language: string | null | undefined): Language {
    const base = language?.trim().toLowerCase().replace('_', '-').split('-')[0];
    return base === 'fr' || base === 'es' ? base : DEFAULT_LANGUAGE;
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
}

export function t(key: CatalogKey, replacements: Record<string, string | number> = {}): string {
    const language = currentLanguage();
    const template = translations[language][key] ?? translations[DEFAULT_LANGUAGE][key] ?? key;
    return Object.entries(replacements).reduce(
        (text, [name, value]) => text.split(`{${name}}`).join(String(value)),
        template
    );
}
