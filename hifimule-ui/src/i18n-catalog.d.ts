declare module '@hifimule/i18n-catalog' {
    // Locale-keyed map. Which locales are actually *supported* is owned by the
    // `Language` union in i18n.ts; the catalog itself is just a string-keyed
    // JSON object, so locales can be looked up dynamically.
    const catalog: Record<string, Record<string, string>>;
    export default catalog;
}
