//! Ulanzi-Pixeluhr (TC001) mit AWTRIX 3 im eigenen Netz: zeigt die [`Scene`] als
//! Text über die ganze Anzeige und räumt ihn danach wieder ab.
//!
//! Lokale HTTP-API: `POST http://<host>/api/notify` legt eine Benachrichtigung
//! an, `hold` lässt sie stehen und `stack: false` ersetzt die oberste, statt
//! sich hinten anzustellen. `POST /api/notify/dismiss` entfernt die oberste. Die
//! Antwort ist der Text „OK“, kein JSON.

use std::time::Duration;

use serde_json::{json, Value};

use crate::display::Scene;
use crate::output::{scene_color, Device};

const TIMEOUT: Duration = Duration::from_secs(3);

/// Ein Aufruf an die Uhr
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Notify(Value),
    Dismiss,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into()
}

/// Einen Aufruf senden
fn send(host: &str, call: Call) -> Result<(), String> {
    let agent = agent();
    let reply = match call {
        Call::Notify(body) => agent.post(&format!("http://{host}/api/notify")).send_json(&body),
        Call::Dismiss => agent.post(&format!("http://{host}/api/notify/dismiss")).send_empty(),
    };
    reply.map(|_| ()).map_err(|e| format!("AWTRIX nicht erreichbar: {e}"))
}

/// Antwort von `/api/stats` prüfen
fn check_reply(v: &Value) -> Result<(), String> {
    if v.get("uid").is_some() {
        Ok(())
    } else {
        Err("AWTRIX nicht erreichbar: unerwartete Antwort".into())
    }
}

/// Uhr erreichbar? Fragt die Kennzahlen der Firmware ab.
pub fn check(host: &str) -> Result<(), String> {
    let v: Value = agent()
        .get(&format!("http://{host}/api/stats"))
        .call()
        .map_err(|e| format!("AWTRIX nicht erreichbar: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|_| "AWTRIX nicht erreichbar: unerwartete Antwort".to_string())?;
    check_reply(&v)
}

/// Benachrichtigung zur Szene, `None` bei `Idle`
fn notification(scene: &Scene) -> Option<Value> {
    let c = scene_color(scene)?;
    let text = match scene {
        Scene::Idle => return None,
        Scene::Live { .. } => "ON AIR".to_string(),
        Scene::Timer { minutes, .. } if *minutes > 99 => "99+ MIN".to_string(),
        Scene::Timer { minutes, .. } => format!("{minutes:02} MIN"),
        Scene::Focus { .. } => "FOKUS".to_string(),
    };
    let mut body = json!({
        "text": text,
        "color": [c.r, c.g, c.b],
        "hold": true,
        "stack": false,
        "noScroll": true,
        "center": true,
    });
    if matches!(scene, Scene::Live { .. }) {
        body["wakeup"] = json!(true);
    }
    Some(body)
}

/// Zustand einer Uhr: was zu sehen ist und ob etwas abzuräumen ist
#[derive(Debug, Default)]
pub struct Session {
    host: Option<String>,
    /// Zuletzt sicher gezeigte Szene
    shown: Option<Scene>,
    /// Eine Benachrichtigung von MicLED ist womöglich zu sehen (auch nach einem
    /// Absturz oder wenn das Senden in den Timeout lief)
    dirty: bool,
}

impl Device for Session {
    type Cfg = String;

    fn apply(&mut self, host: Option<&String>, scene: &Scene) -> Result<(), String> {
        self.apply_with(host.map(String::as_str), scene, &mut send)
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
        send: &mut impl FnMut(&str, Call) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.host.is_some() && self.host.as_deref() != host {
            if let Some(old) = self.host.clone() {
                if let Err(e) = self.clear(&old, send) {
                    log::warn!("alte AWTRIX-Uhr nicht abgeräumt: {e}");
                }
            }
            *self = Session::default();
        }
        let Some(host) = host else { return Ok(()) };
        if self.host.is_none() {
            // ein nach einem Absturz stehengebliebenes „ON AIR“ wird aufgeräumt
            self.dirty = true;
            self.host = Some(host.to_string());
        }
        let Some(body) = notification(scene) else { return self.clear(host, send) };
        if self.shown.as_ref() == Some(scene) {
            return Ok(());
        }
        self.dirty = true;
        self.shown = None;
        send(host, Call::Notify(body))?;
        self.shown = Some(scene.clone());
        Ok(())
    }

    fn clear(&mut self, host: &str, send: &mut impl FnMut(&str, Call) -> Result<(), String>) -> Result<(), String> {
        if self.dirty {
            // das Abräumen kann auch ohne Antwort angekommen sein
            self.shown = None;
            send(host, Call::Dismiss)?;
            self.dirty = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED };
    const CAM: Scene = Scene::Live { camera: true, color: Rgb::MAGENTA };

    fn timer(minutes: u32) -> Scene {
        Scene::Timer { minutes, color: Rgb::RED }
    }

    /// Nimmt Aufrufe auf
    #[derive(Default)]
    struct Fake {
        log: Vec<(String, String)>,
        fail: bool,
    }

    impl Fake {
        fn send(&mut self) -> impl FnMut(&str, Call) -> Result<(), String> + '_ {
            move |host, call| {
                if self.fail {
                    return Err("AWTRIX nicht erreichbar: aus".into());
                }
                let entry = match call {
                    Call::Notify(v) => v["text"].as_str().unwrap().to_string(),
                    Call::Dismiss => "dismiss".into(),
                };
                self.log.push((host.to_string(), entry));
                Ok(())
            }
        }

        fn take(&mut self) -> Vec<String> {
            self.log.drain(..).map(|(_, e)| e).collect()
        }
    }

    /// Session, die ihr erstes Abräumen hinter sich hat
    fn settled(f: &mut Fake) -> Session {
        let mut s = Session::default();
        s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).unwrap();
        f.take();
        s
    }

    #[test]
    fn stats_reply_needs_uid() {
        assert!(check_reply(&json!({"uid": "awtrix_123", "bat": 90})).is_ok());
        assert_eq!(check_reply(&json!({"foo": 1})).unwrap_err(), "AWTRIX nicht erreichbar: unerwartete Antwort");
    }

    #[test]
    fn live_is_held_full_screen_in_its_color_and_wakes_the_display() {
        let v = notification(&CAM).unwrap();
        assert_eq!(
            v,
            json!({
                "text": "ON AIR",
                "color": [Rgb::MAGENTA.r, Rgb::MAGENTA.g, Rgb::MAGENTA.b],
                "hold": true,
                "stack": false,
                "noScroll": true,
                "center": true,
                "wakeup": true,
            })
        );
    }

    #[test]
    fn timer_shows_the_minutes_left() {
        assert_eq!(notification(&timer(7)).unwrap()["text"], "07 MIN");
        assert_eq!(notification(&timer(25)).unwrap()["text"], "25 MIN");
        assert_eq!(notification(&timer(99)).unwrap()["text"], "99 MIN");
        assert_eq!(notification(&timer(100)).unwrap()["text"], "99+ MIN");
        assert!(notification(&timer(25)).unwrap().get("wakeup").is_none());
        assert_eq!(notification(&timer(25)).unwrap()["hold"], true);
    }

    #[test]
    fn focus_and_idle() {
        let v = notification(&Scene::Focus { color: Rgb::YELLOW }).unwrap();
        assert_eq!(v["text"], "FOKUS");
        assert_eq!(v["color"], json!([Rgb::YELLOW.r, Rgb::YELLOW.g, Rgb::YELLOW.b]));
        assert!(v.get("wakeup").is_none());
        assert_eq!(notification(&Scene::Idle), None);
    }

    #[test]
    fn first_idle_clears_once() {
        let mut f = Fake::default();
        let mut s = Session::default();
        s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["dismiss"], "altes „ON AIR“ nach einem Absturz");
        s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn same_scene_is_not_sent_again_and_ends_with_a_dismiss() {
        let mut f = Fake::default();
        let mut s = settled(&mut f);
        s.apply_with(Some("u"), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["ON AIR"]);
        s.apply_with(Some("u"), &LIVE, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
        s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["dismiss"]);
        s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn a_new_minute_is_sent() {
        let mut f = Fake::default();
        let mut s = settled(&mut f);
        s.apply_with(Some("u"), &timer(25), &mut f.send()).unwrap();
        s.apply_with(Some("u"), &timer(24), &mut f.send()).unwrap();
        assert_eq!(f.take(), ["25 MIN", "24 MIN"]);
    }

    #[test]
    fn failures_are_retried() {
        let mut f = Fake::default();
        let mut s = settled(&mut f);
        f.fail = true;
        assert!(s.apply_with(Some("u"), &LIVE, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some("u"), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["ON AIR"], "Senden wird wiederholt");
        f.fail = true;
        assert!(s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["dismiss"], "Abräumen wird wiederholt");
    }

    #[test]
    fn a_notification_without_reply_is_still_cleared() {
        let mut f = Fake::default();
        let mut s = settled(&mut f);
        f.fail = true;
        assert!(s.apply_with(Some("u"), &LIVE, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["dismiss"], "kann angekommen sein");
    }

    #[test]
    fn the_same_scene_is_sent_again_after_a_dismiss_without_reply() {
        let mut f = Fake::default();
        let mut s = settled(&mut f);
        s.apply_with(Some("u"), &LIVE, &mut f.send()).unwrap();
        f.take();
        f.fail = true;
        assert!(s.apply_with(Some("u"), &Scene::Idle, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some("u"), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["ON AIR"], "die Uhr kann schon leer sein");
    }

    #[test]
    fn changing_or_clearing_the_address_clears_the_old_clock() {
        let mut f = Fake::default();
        let mut s = Session::default();
        s.apply_with(Some("a"), &LIVE, &mut f.send()).unwrap();
        f.log.clear();
        s.apply_with(Some("b"), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.log, [("a".to_string(), "dismiss".to_string()), ("b".to_string(), "ON AIR".to_string())]);
        f.log.clear();
        s.apply_with(None, &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.log, [("b".to_string(), "dismiss".to_string())]);
        assert_eq!(s.current(), None);
    }

    #[test]
    fn a_new_address_is_cleared_once_too() {
        let mut f = Fake::default();
        let mut s = settled(&mut f);
        s.apply_with(Some("b"), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.log, [("b".to_string(), "dismiss".to_string())]);
    }

    #[test]
    fn restore_without_address_sends_nothing() {
        let mut s = Session { host: None, shown: None, dirty: true };
        assert!(s.restore().is_ok());
    }

    #[test]
    fn without_address_nothing_is_sent() {
        let mut f = Fake::default();
        let mut s = Session::default();
        s.apply_with(None, &LIVE, &mut f.send()).unwrap();
        s.apply_with(None, &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.log.is_empty());
    }
}
