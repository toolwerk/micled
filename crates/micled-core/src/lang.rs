//! Sprache der Texte, die der Kern an die App gibt (Fehlermeldungen, Text auf
//! Pixoo und AWTRIX-Uhr). Die App stellt sie beim Start nach macOS ein, Tests
//! und CLI bleiben beim Standard Deutsch.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        0 => Lang::De,
        _ => Lang::En,
    }
}

/// Sprachcode von macOS (`de`, `de-AT`, `en-GB`, `fr` …): Deutsch nur für
/// `de…`, alles andere zeigt Englisch
pub fn from_code(code: &str) -> Lang {
    if code.trim().to_ascii_lowercase().starts_with("de") {
        Lang::De
    } else {
        Lang::En
    }
}

/// Wählt den Text zur Sprache
pub fn pick<T>(lang: Lang, de: T, en: T) -> T {
    match lang {
        Lang::De => de,
        Lang::En => en,
    }
}

/// Text in der eingestellten Sprache, als `String`:
/// `tr!("Pixoo meldet Fehler {code}", "Pixoo reports error {code}")`
#[macro_export]
macro_rules! tr {
    ($de:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $crate::lang::current() {
            $crate::lang::Lang::De => format!($de $(, $arg)*),
            $crate::lang::Lang::En => format!($en $(, $arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_codes_give_german() {
        for code in ["de", "de-DE", "de-AT", "de_CH", "DE", " de"] {
            assert_eq!(from_code(code), Lang::De, "{code}");
        }
    }

    #[test]
    fn every_other_code_gives_english() {
        for code in ["en", "en-GB", "fr", "es-ES", "", "dk"] {
            assert_eq!(from_code(code), Lang::En, "{code}");
        }
    }

    #[test]
    fn pick_follows_the_language() {
        assert_eq!(pick(Lang::De, "Fehler", "Error"), "Fehler");
        assert_eq!(pick(Lang::En, "Fehler", "Error"), "Error");
    }

    #[test]
    fn default_is_german() {
        // Kein Test ruft `set`, Tests laufen parallel
        assert_eq!(current(), Lang::De);
        let code = 7;
        assert_eq!(tr!("Fehler {code}", "Error {code}"), "Fehler 7");
    }
}
