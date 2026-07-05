use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

pub const DEFAULT_LANGUAGE: &str = "en";
pub const CATALOG_JSON: &str = include_str!("../catalog.json");

static CATALOG: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(CATALOG_JSON).expect("embedded i18n catalog must be valid JSON")
});

pub fn system_language() -> String {
    std::env::var("HIFIMULE_LANG")
        .map(|value| normalize_language(&value))
        .ok()
        .or_else(system_locale)
        .or_else(posix_locale)
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string())
}

fn posix_locale() -> Option<String> {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok())
        .map(|value| normalize_language(&value))
}

pub fn normalize_language(language: &str) -> String {
    let lower = language.trim().to_ascii_lowercase().replace('_', "-");
    match lower.split('-').next().unwrap_or(DEFAULT_LANGUAGE) {
        "fr" => "fr".to_string(),
        "es" => "es".to_string(),
        "de" => "de".to_string(),
        _ => DEFAULT_LANGUAGE.to_string(),
    }
}

#[cfg(windows)]
fn system_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    const LOCALE_NAME_MAX_LENGTH: usize = 85;
    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    let len = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 1 {
        return None;
    }

    let locale = String::from_utf16_lossy(&buffer[..(len as usize - 1)]);
    Some(normalize_language(&locale))
}

#[cfg(not(windows))]
fn system_locale() -> Option<String> {
    posix_locale()
}

pub fn t(key: &str) -> String {
    translate(&system_language(), key)
}

pub fn tf(key: &str, replacements: &[(&str, &str)]) -> String {
    interpolate(translate(&system_language(), key), replacements)
}

pub fn translate(language: &str, key: &str) -> String {
    let language = normalize_language(language);
    lookup(&language, key)
        .or_else(|| lookup(DEFAULT_LANGUAGE, key))
        .unwrap_or(key)
        .to_string()
}

pub fn translate_with(language: &str, key: &str, replacements: &[(&str, &str)]) -> String {
    interpolate(translate(language, key), replacements)
}

fn lookup(language: &str, key: &str) -> Option<&'static str> {
    CATALOG.get(language)?.get(key)?.as_str()
}

fn interpolate(mut template: String, replacements: &[(&str, &str)]) -> String {
    let replacements: HashMap<&str, &str> = replacements.iter().copied().collect();
    for (key, value) in replacements {
        template = template.replace(&format!("{{{key}}}"), value);
    }
    template
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_english_for_unknown_language() {
        assert_eq!(translate("zz", "tray.quit"), "Quit");
    }

    #[test]
    fn translates_german_keys() {
        assert_eq!(translate("de-DE", "tray.quit"), "Beenden");
    }

    #[test]
    fn translates_french_keys() {
        assert_eq!(translate("fr-FR", "tray.quit"), "Quitter");
    }

    #[test]
    fn translates_spanish_keys() {
        assert_eq!(translate("es-ES", "tray.quit"), "Salir");
    }

    #[test]
    fn interpolates_values() {
        assert_eq!(
            translate_with("en", "tray.tooltip.found", &[("name", "iPod")]),
            "HifiMule: Found iPod"
        );
    }

    #[test]
    fn interpolates_german_values() {
        assert_eq!(
            translate_with(
                "de",
                "basket.sync.file_counter",
                &[("completed", "3"), ("total", "10")]
            ),
            "3 von 10 Dateien"
        );
    }
}
