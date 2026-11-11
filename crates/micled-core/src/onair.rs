//! „Auf Sendung“ nach außen melden, z. B. an einen Home-Assistant-Webhook
//! (`http://homeassistant.local:8123/api/webhook/<id>`), der ein Türschild
//! oder eine Lampe im Flur schaltet.
//!
//! Bei jedem Wechsel geht ein POST mit JSON an die eingetragene Adresse:
//! `{"on_air": true, "camera": false, "apps": ["us.zoom.xos"]}`

use std::time::Duration;

use serde_json::{json, Value};

use crate::tr;

/// Meldung im Menü, wenn die eingetragene Adresse nicht passt.
pub fn invalid_url() -> String {
    tr!("Webhook-Adresse ungültig", "Webhook address invalid")
}

/// Prüft nur die Form der Adresse: `http://` oder `https://`, danach ein Server,
/// keine Leerzeichen. Ob der Server antwortet, zeigt erst der Versand.
/// Eine leere Adresse ist ungültig; „leer = aus“ prüft der Aufrufer.
pub fn valid_url(url: &str) -> bool {
    let url = url.trim();
    if url.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    let rest = match lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) {
        Some(r) => r,
        None => return false,
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(inner) = hostport.strip_prefix('[') {
        match inner.split_once(']') {
            Some((ip, _)) => ip,
            None => return false,
        }
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    !host.is_empty()
}

pub fn payload(on_air: bool, camera: bool, apps: &[String]) -> Value {
    json!({ "on_air": on_air, "camera": camera, "apps": apps })
}

pub fn send(url: &str, body: &Value) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .into();
    agent
        .post(url.trim())
        .send_json(body)
        .map(|_| ())
        .map_err(|e| tr!("Auf-Sendung-Meldung fehlgeschlagen: {e}", "On-air notification failed: {e}", e = e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_url_accepts_real_addresses() {
        for u in [
            "https://homeassistant.local:8123/api/webhook/x",
            "HTTP://host",
            "  http://host  ",
            "http://user:pw@host/x",
            "http://[::1]:8123/x",
        ] {
            assert!(valid_url(u), "{u}");
        }
    }

    #[test]
    fn valid_url_rejects_the_rest() {
        for u in ["Hmm.", "", "http://", "https://:8123/x", "ftp://host", "http:// host", "http://@/x", "http://[]/"] {
            assert!(!valid_url(u), "{u}");
        }
    }

    #[test]
    fn payload_format() {
        let p = payload(true, false, &["us.zoom.xos".into()]);
        assert_eq!(p.to_string(), r#"{"apps":["us.zoom.xos"],"camera":false,"on_air":true}"#);
    }
}
