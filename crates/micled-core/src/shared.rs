//! Gemeinsame Statusdatei für alle Programme, die die Lampe setzen.
//!
//! Pfad: `~/Library/Application Support/net.staude.luxafor/state.json`
//! Inhalt: `{"color":"00ff00","source":"streamdeck","updated":1730750000}`
//!
//! Das Stream-Deck-Plugin und MicLED schreiben hier die zuletzt gewählte
//! „ruhende“ Farbe hinein (feste Farbe oder `000000` für aus). Blinken und
//! Muster werden nicht eingetragen. Die vorübergehende Mikrofarbe auch nicht.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::color::Rgb;

pub const SOURCE: &str = "micled";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedState {
    pub color: Rgb,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub updated: u64,
}

pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/net.staude.luxafor/state.json"))
}

pub fn read(path: &PathBuf) -> Option<SharedState> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write(path: &PathBuf, color: Rgb) -> std::io::Result<SharedState> {
    let state = SharedState {
        color,
        source: SOURCE.into(),
        updated: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // atomar schreiben: erst temporär, dann umbenennen
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(&state)?)?;
    fs::rename(&tmp, path)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read() {
        let dir = std::env::temp_dir().join(format!("micled-test-{}", std::process::id()));
        let path = dir.join("state.json");
        write(&path, Rgb::GREEN).unwrap();
        let s = read(&path).unwrap();
        assert_eq!(s.color, Rgb::GREEN);
        assert_eq!(s.source, "micled");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reads_plugin_format() {
        let s: SharedState = serde_json::from_str(r#"{"color":"FF0000","source":"streamdeck"}"#).unwrap();
        assert_eq!(s.color, Rgb::RED);
    }
}
