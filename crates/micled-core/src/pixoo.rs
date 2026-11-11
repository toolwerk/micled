//! Divoom Pixoo 64 im eigenen Netz: zeigt das Bild zur [`Scene`] und stellt
//! danach den Kanal wieder her, der vorher lief (z. B. die Uhr).
//!
//! Lokale HTTP-API: `POST http://<host>/post` mit JSON `{"Command": …}`, die
//! Antwort enthält `error_code` (0 = gut). Bilder gehen per `Draw/SendHttpGif`
//! mit fortlaufender `PicID`. Wird sie zu groß, hängt das Gerät, deshalb wird
//! die Zählung vorher mit `Draw/ResetHttpGifId` zurückgesetzt.

use std::time::Duration;

use base64::Engine as _;
use serde_json::{json, Value};

use crate::display::{self, Scene};
use crate::output::Device;
use crate::tr;

const TIMEOUT: Duration = Duration::from_secs(3);
/// Ab dieser `PicID` wird die Zählung zurückgesetzt
const MAX_PIC_ID: u32 = 32;
/// Kanal, falls das Gerät keinen meldet (0 = Uhr)
const DEFAULT_CHANNEL: u64 = 0;

/// Adresse aus den Einstellungen bereinigen: `http://`, `https://` und `/` am
/// Ende entfernen. Leer = aus.
pub fn normalize(host: &str) -> Option<String> {
    let h = host.trim();
    let h = h.strip_prefix("http://").or_else(|| h.strip_prefix("https://")).unwrap_or(h);
    let h = h.trim_end_matches('/').trim();
    (!h.is_empty()).then(|| h.to_string())
}

fn url(host: &str) -> String {
    format!("http://{host}/post")
}

/// Antwort des Geräts prüfen
fn check_reply(v: &Value) -> Result<(), String> {
    match v["error_code"].as_i64() {
        Some(0) => Ok(()),
        Some(code) => Err(tr!("Pixoo meldet Fehler {code}", "Pixoo reports error {code}")),
        None => Err(tr!("Pixoo nicht erreichbar: unerwartete Antwort", "Pixoo unreachable: unexpected reply")),
    }
}

/// Einen Befehl senden und die Antwort prüfen
fn post(host: &str, body: Value) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
    let v: Value = agent
        .post(&url(host))
        .send_json(&body)
        .map_err(|e| tr!("Pixoo nicht erreichbar: {e}", "Pixoo unreachable: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|_| tr!("Pixoo nicht erreichbar: Antwort ist kein JSON", "Pixoo unreachable: reply is not JSON"))?;
    check_reply(&v)?;
    Ok(v)
}

/// Gerät erreichbar? Fragt den laufenden Kanal ab.
pub fn check(host: &str) -> Result<(), String> {
    post(host, json!({ "Command": "Channel/GetIndex" })).map(|_| ())
}

/// Befehl für ein Bild
fn gif(scene: &Scene, pic_id: u32) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(display::render(scene));
    json!({
        "Command": "Draw/SendHttpGif",
        "PicNum": 1,
        "PicWidth": display::SIZE,
        "PicOffset": 0,
        "PicID": pic_id,
        "PicSpeed": 1000,
        "PicData": data,
    })
}

/// Zustand eines Geräts: welcher Kanal vorher lief und ob gerade ein Bild von
/// MicLED zu sehen ist.
#[derive(Debug, Default)]
pub struct Session {
    host: Option<String>,
    /// Kanal von vor dem ersten Bild
    saved_channel: Option<u64>,
    /// Zuletzt sicher gezeigtes Bild
    shown: Option<Scene>,
    /// Ein Bild von MicLED ist womöglich zu sehen (auch, wenn das Senden in
    /// den Timeout lief); dann muss der Kanal zurückgestellt werden
    dirty: bool,
    /// Nächste `PicID`, 0 = Zählung noch nicht zurückgesetzt
    pic_id: u32,
}

impl Device for Session {
    type Cfg = String;

    fn apply(&mut self, host: Option<&String>, scene: &Scene) -> Result<(), String> {
        self.apply_with(host.map(String::as_str), scene, &mut post)
    }

    fn restore(&mut self) -> Result<(), String> {
        let host = self.host.clone();
        self.apply(host.as_ref(), &Scene::Idle)
    }

    fn current(&self) -> Option<&String> {
        self.host.as_ref()
    }
}

impl Session {
    fn apply_with(
        &mut self,
        host: Option<&str>,
        scene: &Scene,
        send: &mut impl FnMut(&str, Value) -> Result<Value, String>,
    ) -> Result<(), String> {
        if self.host.is_some() && self.host.as_deref() != host {
            if let Some(old) = self.host.clone() {
                if let Err(e) = self.idle(&old, send) {
                    log::warn!("alten Pixoo nicht zurückgestellt: {e}");
                }
            }
            *self = Session::default();
        }
        let Some(host) = host else { return Ok(()) };
        self.host = Some(host.to_string());
        if *scene == Scene::Idle {
            return self.idle(host, send);
        }
        if self.shown.as_ref() == Some(scene) {
            return Ok(());
        }
        if self.saved_channel.is_none() {
            let v = send(host, json!({ "Command": "Channel/GetIndex" }))?;
            self.saved_channel = Some(v["SelectIndex"].as_u64().unwrap_or(DEFAULT_CHANNEL));
        }
        if self.pic_id == 0 || self.pic_id >= MAX_PIC_ID {
            send(host, json!({ "Command": "Draw/ResetHttpGifId" }))?;
            self.pic_id = 1;
        }
        self.dirty = true;
        self.shown = None;
        let id = self.pic_id;
        self.pic_id += 1;
        send(host, gif(scene, id))?;
        self.shown = Some(scene.clone());
        Ok(())
    }

    fn idle(&mut self, host: &str, send: &mut impl FnMut(&str, Value) -> Result<Value, String>) -> Result<(), String> {
        if self.dirty {
            let channel = self.saved_channel.unwrap_or(DEFAULT_CHANNEL);
            send(host, json!({ "Command": "Channel/SetIndex", "SelectIndex": channel }))?;
            self.dirty = false;
            self.shown = None;
            self.saved_channel = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    #[test]
    fn normalizes_addresses() {
        assert_eq!(normalize(" 192.168.1.50 "), Some("192.168.1.50".into()));
        assert_eq!(normalize("http://192.168.1.50/"), Some("192.168.1.50".into()));
        assert_eq!(normalize("https://pixoo.local//"), Some("pixoo.local".into()));
        assert_eq!(normalize("  "), None);
        assert_eq!(normalize("http://"), None);
        assert_eq!(url("192.168.1.50"), "http://192.168.1.50/post");
    }

    #[test]
    fn reply_needs_error_code_zero() {
        assert!(check_reply(&json!({"error_code": 0})).is_ok());
        assert_eq!(check_reply(&json!({"error_code": 1})).unwrap_err(), "Pixoo meldet Fehler 1");
        assert!(check_reply(&json!({"foo": 1})).is_err());
    }

    #[test]
    fn gif_command_format() {
        let live = Scene::Live { camera: false, color: Rgb::RED, until: None };
        let v = gif(&live, 3);
        assert_eq!(v["Command"], "Draw/SendHttpGif");
        assert_eq!(v["PicWidth"], 64);
        assert_eq!(v["PicID"], 3);
        let data = base64::engine::general_purpose::STANDARD.decode(v["PicData"].as_str().unwrap()).unwrap();
        assert_eq!(data, display::render(&live));
    }

    /// Nimmt Befehle auf und antwortet wie ein Gerät auf Kanal 3
    #[derive(Default)]
    struct Fake {
        log: Vec<(String, String)>,
        fail: bool,
        channel: Option<u64>,
    }

    impl Fake {
        fn send(&mut self) -> impl FnMut(&str, Value) -> Result<Value, String> + '_ {
            move |host, body| {
                let cmd = body["Command"].as_str().unwrap().to_string();
                let entry = match cmd.as_str() {
                    "Draw/SendHttpGif" => format!("gif {}", body["PicID"]),
                    "Channel/SetIndex" => format!("set {}", body["SelectIndex"]),
                    "Channel/GetIndex" => "get".into(),
                    "Draw/ResetHttpGifId" => "reset".into(),
                    c => c.to_string(),
                };
                if self.fail {
                    return Err("Pixoo nicht erreichbar: aus".into());
                }
                self.log.push((host.to_string(), entry));
                Ok(match self.channel {
                    Some(c) => json!({"error_code": 0, "SelectIndex": c}),
                    None => json!({"error_code": 0}),
                })
            }
        }

        fn take(&mut self) -> Vec<String> {
            self.log.drain(..).map(|(_, e)| e).collect()
        }
    }

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED, until: None };
    const CAM: Scene = Scene::Live { camera: true, color: Rgb::MAGENTA, until: None };

    #[test]
    fn first_image_saves_channel_and_resets_ids() {
        let mut f = Fake { channel: Some(3), ..Fake::default() };
        let mut s = Session::default();
        s.apply_with(Some("p"), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["get", "reset", "gif 1"]);
        s.apply_with(Some("p"), &CAM, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["gif 2"], "Kanal nur beim ersten Bild abfragen");
        s.apply_with(Some("p"), &CAM, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "gleiches Bild nicht erneut senden");
        s.apply_with(Some("p"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["set 3"]);
        s.apply_with(Some("p"), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn unknown_channel_falls_back_to_clock() {
        let mut f = Fake::default();
        let mut s = Session::default();
        s.apply_with(Some("p"), &LIVE, &mut f.send()).unwrap();
        s.apply_with(Some("p"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take().last().unwrap(), "set 0");
    }

    #[test]
    fn pic_ids_are_reset_before_the_device_hangs() {
        let mut f = Fake { channel: Some(1), ..Fake::default() };
        let mut s = Session::default();
        for m in 0..40 {
            let scene = Scene::Timer { minutes: m, color: Rgb::RED };
            s.apply_with(Some("p"), &scene, &mut f.send()).unwrap();
        }
        let log = f.take();
        let max = log.iter().filter_map(|e| e.strip_prefix("gif ")).map(|n| n.parse::<u32>().unwrap()).max();
        assert!(max.unwrap() < MAX_PIC_ID);
        assert_eq!(log.iter().filter(|e| *e == "reset").count(), 2);
        let i = log.iter().position(|e| e == "gif 31").unwrap();
        assert_eq!(log[i + 1..i + 3], ["reset", "gif 1"]);
    }

    #[test]
    fn failure_keeps_state_for_the_retry() {
        let mut f = Fake { channel: Some(2), fail: true, ..Fake::default() };
        let mut s = Session::default();
        assert!(s.apply_with(Some("p"), &LIVE, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some("p"), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["get", "reset", "gif 1"]);
        f.fail = true;
        assert!(s.apply_with(Some("p"), &Scene::Idle, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some("p"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["set 2"], "Zurückstellen wird wiederholt");
    }

    #[test]
    fn changing_or_clearing_the_address_restores_the_old_device() {
        let mut f = Fake { channel: Some(4), ..Fake::default() };
        let mut s = Session::default();
        s.apply_with(Some("a"), &LIVE, &mut f.send()).unwrap();
        f.log.clear();
        s.apply_with(Some("b"), &LIVE, &mut f.send()).unwrap();
        let hosts = std::mem::take(&mut f.log);
        assert_eq!(hosts[0], ("a".into(), "set 4".into()));
        assert!(hosts[1..].iter().all(|(h, _)| h == "b"));
        assert_eq!(hosts[1..].iter().map(|(_, e)| e.as_str()).collect::<Vec<_>>(), ["get", "reset", "gif 1"]);
        s.apply_with(None, &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.log, [("b".to_string(), "set 4".to_string())]);
    }

    #[test]
    fn image_sent_without_reply_is_still_cleared() {
        let mut f = Fake { channel: Some(5), ..Fake::default() };
        let mut s = Session::default();
        let mut calls = 0;
        // GetIndex und Reset gehen durch, beim Bild kommt keine Antwort
        let r = s.apply_with(Some("p"), &LIVE, &mut |h, b| {
            calls += 1;
            if calls == 3 {
                return Err("Pixoo nicht erreichbar: timeout".into());
            }
            f.send()(h, b)
        });
        assert!(r.is_err());
        f.take();
        s.apply_with(Some("p"), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["gif 2"], "Kanal nicht erneut abfragen, ID nicht doppelt");
        let mut s2 = Session::default();
        let mut calls = 0;
        let _ = s2.apply_with(Some("p"), &LIVE, &mut |h, b| {
            calls += 1;
            if calls == 3 {
                return Err("timeout".into());
            }
            f.send()(h, b)
        });
        f.take();
        s2.apply_with(Some("p"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["set 5"], "Bild könnte zu sehen sein, also zurückstellen");
    }

    #[test]
    fn idle_without_image_sends_nothing() {
        let mut f = Fake::default();
        let mut s = Session::default();
        s.apply_with(Some("p"), &Scene::Idle, &mut f.send()).unwrap();
        s.apply_with(None, &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.log.is_empty());
    }
}
