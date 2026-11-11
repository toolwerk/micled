//! Sprache der Texte, die der Kern an die App gibt (Fehlermeldungen, Text auf
//! Pixoo und AWTRIX-Uhr). Die App stellt sie beim Start nach macOS ein, Tests
//! und CLI bleiben beim Standard Deutsch.
//!
//! Deutsch und Englisch stehen direkt im Code (`tr!`). Weitere Sprachen kommen
//! aus `locales.json`, das `scripts/translations.sh` aus `translations/<code>.po`
//! schreibt: je Sprache das deutsche Original als Schlüssel und die Übersetzung.
//! Was dort fehlt, erscheint auf Englisch.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
    /// n-te Sprache aus `locales.json`, nach Code sortiert
    Other(u8),
}

/// Übersetzungen einer Sprache: deutsches Original → Text
pub type Table = HashMap<String, String>;

static CURRENT: AtomicU8 = AtomicU8::new(0);
static LOCALES: OnceLock<Vec<(String, Table)>> = OnceLock::new();

/// Weitere Sprachen, nach Code sortiert
fn locales() -> &'static [(String, Table)] {
    LOCALES.get_or_init(|| parse_locales(include_str!("locales.json")))
}

fn parse_locales(json: &str) -> Vec<(String, Table)> {
    let map: BTreeMap<String, Table> = serde_json::from_str(json).unwrap_or_default();
    let mut v: Vec<(String, Table)> = map.into_iter().map(|(code, table)| (normalize(&code), table)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

pub fn set(lang: Lang) {
    CURRENT.store(encode(lang), Ordering::Relaxed);
}

pub fn current() -> Lang {
    decode(CURRENT.load(Ordering::Relaxed))
}

/// 0 Deutsch, 1 Englisch, 2 + n weitere Sprache n
fn encode(lang: Lang) -> u8 {
    match lang {
        Lang::De => 0,
        Lang::En => 1,
        Lang::Other(n) => n.saturating_add(2),
    }
}

fn decode(v: u8) -> Lang {
    match v {
        0 => Lang::De,
        1 => Lang::En,
        v => Lang::Other(v - 2),
    }
}

fn normalize(code: &str) -> String {
    code.trim().to_ascii_lowercase().replace('_', "-")
}

/// Sprachcode von macOS (`de`, `de-AT`, `en-GB`, `fr` …): Deutsch für `de…`,
/// eine Sprache aus `locales.json`, sonst Englisch
pub fn from_code(code: &str) -> Lang {
    let codes: Vec<&str> = locales().iter().map(|(c, _)| c.as_str()).collect();
    from_code_in(&codes, code)
}

/// `from_code` gegen eine sortierte Liste weiterer Sprachen: erst der ganze
/// Code (`pt-br`), dann der erste Teil (`fr` für `fr-CA`)
fn from_code_in(codes: &[&str], code: &str) -> Lang {
    let code = normalize(code);
    if code.starts_with("de") {
        return Lang::De;
    }
    let first = code.split('-').next().unwrap_or_default();
    codes
        .iter()
        .position(|c| *c == code)
        .or_else(|| codes.iter().position(|c| *c == first))
        .and_then(|n| u8::try_from(n).ok())
        .map_or(Lang::En, Lang::Other)
}

/// Text in einer weiteren Sprache `n`; fehlt sie oder der Text, Englisch
#[doc(hidden)]
pub fn other(n: u8, de: &str, en: &str, args: &[(&str, &dyn Display)]) -> String {
    let table = locales().get(usize::from(n)).map(|(_, t)| t);
    render(table, de, en, args)
}

/// Übersetzung zum deutschen Original aus `table`, sonst `en`, mit den
/// Platzhaltern `{name}` gefüllt
fn render(table: Option<&Table>, de: &str, en: &str, args: &[(&str, &dyn Display)]) -> String {
    let text = table.and_then(|t| t.get(de)).map_or(en, String::as_str);
    fill(text, args)
}

/// Ersetzt `{name}` durch das Argument gleichen Namens, `{{` und `}}` durch
/// eine Klammer. Unbekannte Platzhalter bleiben stehen.
fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('{') {
            if let Some(end) = tail.find('}') {
                let name = &tail[1..end];
                if let Some((_, v)) = args.iter().find(|(n, _)| *n == name) {
                    out.push_str(&v.to_string());
                    rest = &tail[end + 1..];
                    continue;
                }
            }
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// Wählt den Text zur Sprache; nur der gewählte wird erzeugt. Weitere
/// Sprachen bekommen den englischen.
pub fn pick<T>(lang: Lang, de: impl FnOnce() -> T, en: impl FnOnce() -> T) -> T {
    match lang {
        Lang::De => de(),
        Lang::En | Lang::Other(_) => en(),
    }
}

/// Text in der eingestellten Sprache, als `String`. Platzhalter sind benannt
/// und werden ausdrücklich übergeben, Formatangaben gehören ins Argument:
/// `tr!("Pixoo meldet Fehler {code}", "Pixoo reports error {code}", code = code)`.
/// `scripts/translations.sh` liest beide Texte für die Übersetzungsvorlage.
#[macro_export]
macro_rules! tr {
    ($de:literal, $en:literal $(, $name:ident = $arg:expr)* $(,)?) => {
        match $crate::lang::current() {
            $crate::lang::Lang::De => format!($de $(, $name = $arg)*),
            $crate::lang::Lang::En => format!($en $(, $name = $arg)*),
            $crate::lang::Lang::Other(n) => $crate::lang::other(
                n,
                $de,
                $en,
                &[$((stringify!($name), &$arg as &dyn ::std::fmt::Display)),*],
            ),
        }
    };
}

/// Wie `tr!`, aber nur Deutsch oder Englisch und mit allen Formatangaben, für
/// Text auf Pixoo und AWTRIX-Uhr: Deren Pixelschrift kennt nur wenige
/// Buchstaben, weitere Sprachen zeigen dort Englisch.
macro_rules! tr_fixed {
    ($de:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        $crate::lang::pick($crate::lang::current(), || format!($de $(, $arg)*), || format!($en $(, $arg)*))
    };
}
pub(crate) use tr_fixed;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_codes_give_german() {
        for code in ["de", "de-DE", "de-AT", "de_CH", "DE", " de"] {
            assert_eq!(from_code(code), Lang::De, "{code}");
            assert_eq!(from_code_in(&["fr"], code), Lang::De, "{code}");
        }
    }

    #[test]
    fn every_other_code_gives_english() {
        for code in ["en", "en-GB", "fr", "es-ES", "", "dk"] {
            assert_eq!(from_code_in(&[], code), Lang::En, "{code}");
        }
    }

    #[test]
    fn listed_codes_give_their_language() {
        let codes = ["es", "fr", "pt-br"];
        assert_eq!(from_code_in(&codes, "fr"), Lang::Other(1));
        assert_eq!(from_code_in(&codes, "fr-CA"), Lang::Other(1));
        assert_eq!(from_code_in(&codes, "pt_BR"), Lang::Other(2));
        assert_eq!(from_code_in(&codes, "pt"), Lang::En);
        assert_eq!(from_code_in(&codes, "en"), Lang::En);
    }

    #[test]
    fn language_survives_the_atomic() {
        // nur die Umrechnung, ohne `set`: Tests laufen parallel
        for (lang, v) in [(Lang::De, 0u8), (Lang::En, 1), (Lang::Other(0), 2), (Lang::Other(5), 7)] {
            assert_eq!(encode(lang), v);
            assert_eq!(decode(v), lang);
        }
    }

    #[test]
    fn locales_file_parses() {
        let parsed = parse_locales(include_str!("locales.json"));
        assert!(parsed.windows(2).all(|w| w[0].0 < w[1].0));
        let sample = parse_locales(r#"{"fr": {"Fehler {code}": "Erreur {code}"}, "ES": {}, "zh-HK": {}, "zh-Hans": {}}"#);
        assert_eq!(sample.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["es", "fr", "zh-hans", "zh-hk"]);
    }

    #[test]
    fn translation_fills_named_placeholders() {
        let table: Table = [("Pixoo {host} meldet Fehler {code}".to_string(), "Pixoo {host} : erreur {code}".to_string())].into();
        let (host, code) = ("10.0.0.2", 7);
        let args: [(&str, &dyn Display); 2] = [("host", &host), ("code", &code)];
        assert_eq!(render(Some(&table), "Pixoo {host} meldet Fehler {code}", "Pixoo {host} reports error {code}", &args), "Pixoo 10.0.0.2 : erreur 7");
    }

    #[test]
    fn missing_translation_falls_back_to_english() {
        let table = Table::new();
        let e = "timeout";
        let args: [(&str, &dyn Display); 1] = [("e", &e)];
        assert_eq!(render(Some(&table), "Fehler: {e}", "Error: {e}", &args), "Error: timeout");
        assert_eq!(render(None, "Fehler: {e}", "Error: {e}", &args), "Error: timeout");
    }

    #[test]
    fn translation_may_reorder_placeholders() {
        let table: Table = [("{a} vor {b}".to_string(), "{b} après {a}".to_string())].into();
        let args: [(&str, &dyn Display); 2] = [("a", &1), ("b", &2)];
        assert_eq!(render(Some(&table), "{a} vor {b}", "{a} before {b}", &args), "2 après 1");
    }

    #[test]
    fn braces_are_escaped_and_unknown_names_kept() {
        let args: [(&str, &dyn Display); 1] = [("x", &5)];
        assert_eq!(fill("{{x}} = {x}, {y} }", &args), "{x} = 5, {y} }");
    }

    #[test]
    fn pick_follows_the_language() {
        // derselbe Weg wie in `tr_fixed!`
        let code = 7;
        assert_eq!(pick(Lang::De, || format!("Fehler {code}"), || format!("Error {code}")), "Fehler 7");
        assert_eq!(pick(Lang::En, || format!("Fehler {code}"), || format!("Error {code}")), "Error 7");
        assert_eq!(pick(Lang::Other(0), || format!("Fehler {code}"), || format!("Error {code}")), "Error 7");
    }

    #[test]
    fn default_is_german() {
        // Kein Test ruft `set`, Tests laufen parallel
        assert_eq!(current(), Lang::De);
        let code = 7;
        assert_eq!(tr!("Fehler {code}", "Error {code}", code = code), "Fehler 7");
        assert_eq!(tr_fixed!("BIS {:02}", "TIL {:02}", 5), "BIS 05");
    }
}
