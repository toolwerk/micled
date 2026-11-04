//! Farben im Format `rrggbb`. Schwarz (`000000`) bedeutet „Lampe aus“.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Rgb = Rgb { r: 0, g: 0, b: 0 };
    pub const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
    pub const GREEN: Rgb = Rgb { r: 0, g: 255, b: 0 };

    /// Liest `ff8800`, `#FF8800` oder `f80`.
    pub fn parse(s: &str) -> Option<Rgb> {
        let named = match s.trim().to_ascii_lowercase().as_str() {
            "red" | "rot" => Some("ff0000"),
            "green" | "grün" | "gruen" => Some("00ff00"),
            "blue" | "blau" => Some("0000ff"),
            "yellow" | "gelb" => Some("ffff00"),
            "white" | "weiß" | "weiss" => Some("ffffff"),
            "cyan" => Some("00ffff"),
            "magenta" => Some("ff00ff"),
            "off" | "aus" | "black" | "schwarz" => Some("000000"),
            _ => None,
        };
        let s = named.unwrap_or(s).trim().trim_start_matches('#');
        let full = match s.len() {
            3 => s.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => s.to_string(),
            _ => return None,
        };
        let byte = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).ok();
        Some(Rgb { r: byte(0)?, g: byte(2)?, b: byte(4)? })
    }

    pub fn hex(&self) -> String {
        format!("{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn is_off(&self) -> bool {
        *self == Rgb::OFF
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Rgb::parse(&s).ok_or_else(|| format!("ungültige Farbe: {s}"))
    }
}

impl From<Rgb> for String {
    fn from(c: Rgb) -> String {
        c.hex()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_formats() {
        assert_eq!(Rgb::parse("ff8800"), Some(Rgb { r: 255, g: 136, b: 0 }));
        assert_eq!(Rgb::parse("#FF8800"), Some(Rgb { r: 255, g: 136, b: 0 }));
        assert_eq!(Rgb::parse("f80"), Some(Rgb { r: 255, g: 136, b: 0 }));
        assert_eq!(Rgb::parse("xyz"), None);
        assert_eq!(Rgb::parse("12345"), None);
    }

    #[test]
    fn roundtrip_hex() {
        assert_eq!(Rgb::parse("0a0B0c").unwrap().hex(), "0a0b0c");
    }
}
