//! Befehle von außen, z. B. aus Kurzbefehlen, Raycast oder dem Stream Deck:
//!
//! - `micled://color/ff8800` (auch `micled://farbe/rot`) setzt eine Farbe
//! - `micled://off` schaltet die Lampe aus
//! - `micled://auto/on`, `/off`, `/toggle` schaltet die Automatik
//! - `micled://timer/25` startet einen Konzentrations-Timer, `micled://timer/off` beendet ihn
//! - `micled://focus/on`, `/off` meldet einen Fokus von macOS (für Kurzbefehle-Automationen)

use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    SetColor(Rgb),
    Off,
    /// `None` = umschalten
    Automatic(Option<bool>),
    /// Minuten, 0 = beenden
    Timer(u32),
    Focus(bool),
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
        ("timer", Some("off" | "aus" | "stop")) => Some(Command::Timer(0)),
        ("timer", Some(m)) => m.parse().ok().filter(|&m| (1..=600).contains(&m)).map(Command::Timer),
        ("focus" | "fokus", Some("on" | "an")) => Some(Command::Focus(true)),
        ("focus" | "fokus", Some("off" | "aus")) => Some(Command::Focus(false)),
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
    fn parses_timer_and_focus() {
        assert_eq!(parse("micled://timer/25"), Some(Command::Timer(25)));
        assert_eq!(parse("micled://timer/off"), Some(Command::Timer(0)));
        assert_eq!(parse("micled://timer/0"), None);
        assert_eq!(parse("micled://timer/abc"), None);
        assert_eq!(parse("micled://timer"), None);
        assert_eq!(parse("micled://focus/on"), Some(Command::Focus(true)));
        assert_eq!(parse("micled://fokus/aus"), Some(Command::Focus(false)));
    }

    #[test]
    fn rejects_other_urls() {
        assert_eq!(parse("https://example.com/color/ff0000"), None);
        assert_eq!(parse("micled://blink/ff0000"), None);
        assert_eq!(parse("micled://"), None);
    }
}
