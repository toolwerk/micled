//! Befehle von außen, z. B. aus Kurzbefehlen, Raycast oder dem Stream Deck:
//!
//! - `micled://color/ff8800` (auch `micled://farbe/rot`) setzt eine Farbe
//! - `micled://off` schaltet die Lampe aus
//! - `micled://auto/on`, `/off`, `/toggle` schaltet die Automatik

use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    SetColor(Rgb),
    Off,
    /// `None` = umschalten
    Automatic(Option<bool>),
}

pub fn parse(url: &str) -> Option<Command> {
    let rest = url.trim().strip_prefix("micled://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let verb = parts.next()?.to_ascii_lowercase();
    let arg = parts.next().map(|a| a.replace("%23", "#").to_ascii_lowercase());
    match (verb.as_str(), arg.as_deref()) {
        ("color" | "farbe", Some(c)) => Rgb::parse(c).map(Command::SetColor),
        ("off" | "aus", None) => Some(Command::Off),
        ("auto", Some("on" | "an")) => Some(Command::Automatic(Some(true))),
        ("auto", Some("off" | "aus")) => Some(Command::Automatic(Some(false))),
        ("auto", Some("toggle") | None) => Some(Command::Automatic(None)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(parse("micled://color/ff0000"), Some(Command::SetColor(Rgb::RED)));
        assert_eq!(parse("micled://farbe/Grün"), Some(Command::SetColor(Rgb::GREEN)));
        assert_eq!(parse("micled://color/%23ff0000"), Some(Command::SetColor(Rgb::RED)));
        assert_eq!(parse("micled://color/zzz"), None);
        assert_eq!(parse("micled://color"), None);
    }

    #[test]
    fn parses_off_and_automatic() {
        assert_eq!(parse("micled://off"), Some(Command::Off));
        assert_eq!(parse("micled://off/"), Some(Command::Off));
        assert_eq!(parse("micled://auto/on"), Some(Command::Automatic(Some(true))));
        assert_eq!(parse("micled://auto/aus"), Some(Command::Automatic(Some(false))));
        assert_eq!(parse("micled://auto/toggle"), Some(Command::Automatic(None)));
        assert_eq!(parse("micled://auto"), Some(Command::Automatic(None)));
    }

    #[test]
    fn rejects_other_urls() {
        assert_eq!(parse("https://example.com/color/ff0000"), None);
        assert_eq!(parse("micled://blink/ff0000"), None);
        assert_eq!(parse("micled://"), None);
    }
}
