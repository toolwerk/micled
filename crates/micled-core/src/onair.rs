//! „Auf Sendung“ nach außen melden, z. B. an einen Home-Assistant-Webhook
//! (`http://homeassistant.local:8123/api/webhook/<id>`), der ein Türschild
//! oder eine Lampe im Flur schaltet.
//!
//! Bei jedem Wechsel geht ein POST mit JSON an die eingetragene Adresse:
//! `{"on_air": true, "camera": false, "apps": ["us.zoom.xos"]}`

use std::time::Duration;

use serde_json::{json, Value};

use crate::tr;

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
        .map_err(|e| tr!("Auf-Sendung-Meldung fehlgeschlagen: {e}", "On-air notification failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_format() {
        let p = payload(true, false, &["us.zoom.xos".into()]);
        assert_eq!(p.to_string(), r#"{"apps":["us.zoom.xos"],"camera":false,"on_air":true}"#);
    }
}
