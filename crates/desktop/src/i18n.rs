use serde::Deserialize;
use std::{cell::Cell, collections::BTreeMap, sync::OnceLock};

include!(concat!(env!("OUT_DIR"), "/locales.rs"));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Language {
    pub name: String,
    pub messages: BTreeMap<String, String>,
}

pub fn languages() -> &'static BTreeMap<&'static str, Language> {
    static LANGUAGES: OnceLock<BTreeMap<&str, Language>> = OnceLock::new();
    LANGUAGES.get_or_init(|| {
        SOURCES
            .iter()
            .map(|&(code, source)| {
                let language = serde_json::from_str(source)
                    .unwrap_or_else(|error| panic!("Invalid bundled locale {code}: {error}"));
                (code, language)
            })
            .collect()
    })
}

pub fn normalize(code: &str) -> &'static str {
    languages()
        .get_key_value(code)
        .map_or("en", |(&code, _)| code)
}

pub fn menu_languages() -> impl Iterator<Item = (&'static str, &'static Language)> {
    const FIRST: [&str; 4] = ["en", "de", "fr", "es"];
    let languages = languages();
    FIRST
        .into_iter()
        .filter_map(|code| languages.get(code).map(|language| (code, language)))
        .chain(
            languages.iter().filter_map(|(&code, language)| {
                (!FIRST.contains(&code)).then_some((code, language))
            }),
        )
}

thread_local! {
    // The editor sets the locale at the start of each UI frame.
    static SELECTED: Cell<&'static str> = const { Cell::new("en") };
}

pub fn select(code: &str) {
    SELECTED.set(normalize(code));
}

fn translate<'a>(language: &'a Language, source: &str) -> Option<&'a str> {
    language
        .messages
        .get(source)
        .map(String::as_str)
        .filter(|text| !text.trim().is_empty())
}

pub fn tr(source: &str) -> &str {
    let code = SELECTED.get();
    if code == "en" {
        return source;
    }
    languages()
        .get(code)
        .and_then(|language| translate(language, source))
        .unwrap_or(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_menu_order_is_fixed() {
        for selected in ["en", "de", "fr", "es"] {
            select(selected);
            let codes: Vec<_> = menu_languages().take(4).map(|(code, _)| code).collect();
            assert_eq!(codes, ["en", "de", "fr", "es"]);
        }
        select("en");
    }

    #[test]
    fn bundled_catalogs_use_known_messages() {
        let english = &languages()["en"];
        assert!(!english.messages.is_empty());
        for (&code, language) in languages() {
            assert!(
                !code.is_empty()
                    && code.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'-'),
                "{code}"
            );
            assert!(!language.name.trim().is_empty(), "{code}");
            for (source, message) in &language.messages {
                assert!(
                    english.messages.contains_key(source),
                    "{code}: unknown message {source}"
                );
                assert!(
                    !message.trim().is_empty(),
                    "{code}: omit untranslated message {source}"
                );
                if code == "en" {
                    assert_eq!(source, message);
                }
            }
        }
    }

    #[test]
    fn unsupported_locale_and_missing_messages_fall_back() {
        assert_eq!(normalize("invalid locale"), "en");
        let language: Language =
            serde_json::from_str(r#"{"name":"Test","messages":{"Settings":"Locale text"}}"#)
                .unwrap();
        assert_eq!(translate(&language, "Settings"), Some("Locale text"));
        assert_eq!(translate(&language, "About"), None);
        select("invalid locale");
        assert_eq!(tr("Settings"), "Settings");
    }
}
