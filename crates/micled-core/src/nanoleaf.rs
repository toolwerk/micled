//! Nanoleaf-Lampen (Light Panels, Canvas, Shapes, Elements, Lines, Skylight)
//! über die lokale OpenAPI: Die Lampe zeigt die Farbe der [`Scene`] und bekommt
//! danach An/Aus, Helligkeit und Farbe bzw. Effekt von vorher zurück.
//!
//! HTTP an Port 16021. Koppeln: Einschaltknopf 5–7 Sekunden halten, dann
//! `POST /api/v1/new`. Zustand mit `GET /api/v1/<schlüssel>/`, Schalten mit
//! `PUT …/state`, Effekt mit `PUT …/effects`. Gefunden werden die Geräte per
//! Bonjour (`_nanoleafapi._tcp`).

use std::fmt;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::color::Rgb;
use crate::display::Scene;
use crate::output::{scene_color, Device, Keyed};
use crate::tr;

pub const PORT: u16 = 16021;
const TIMEOUT: Duration = Duration::from_secs(3);
const SERVICE: &str = "_nanoleafapi._tcp.local.";
/// So lange sammelt die Suche Antworten
const SCAN_WAIT: Duration = Duration::from_secs(3);

pub fn pair_hint() -> String {
    tr!("Einschaltknopf 5–7 Sekunden halten, dann innerhalb von 30 Sekunden koppeln", "Hold the power button for 5–7 seconds, then pair within 30 seconds")
}

/// Gekoppeltes Nanoleaf-Gerät
#[derive(Clone, PartialEq, Eq)]
pub struct Nanoleaf {
    /// Adresse mit Port, siehe [`normalize`]
    pub host: String,
    /// Schlüssel vom Koppeln
    pub key: String,
}

impl fmt::Debug for Nanoleaf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nanoleaf").field("host", &self.host).field("key", &"…").finish()
    }
}

impl Keyed for Nanoleaf {
    fn address(&self) -> &str {
        &self.host
    }
}

/// Adresse aus den Einstellungen bereinigen wie [`crate::pixoo::normalize`],
/// ohne Port kommt `:16021` dazu. Leer = aus.
pub fn normalize(host: &str) -> Option<String> {
    let h = crate::pixoo::normalize(host)?;
    Some(if h.contains(':') { h } else { format!("{h}:{PORT}") })
}

/// Farbton (0–360), Sättigung und Helligkeit (je 0–100). `OFF` ergibt `None`.
pub fn hsv(c: Rgb) -> Option<(u64, u64, u64)> {
    let (r, g, b) = (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
    let max = r.max(g).max(b);
    if max == 0.0 {
        return None;
    }
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let pct = |v: f64| (v * 100.0).round() as u64;
    Some(((h.round() as u64) % 360, pct(d / max), pct(max)))
}

/// Befehl für die Farbe `c` (`OFF` schaltet aus), ohne Überblendung
fn color_body(c: Rgb) -> Value {
    match hsv(c) {
        None => json!({ "on": { "value": false } }),
        Some((h, s, v)) => json!({
            "on": { "value": true },
            "hue": { "value": h },
            "sat": { "value": s },
            "brightness": { "value": v, "duration": 0 },
        }),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Look {
    /// Gespeicherter Effekt (Name ohne führendes `*`)
    Effect(String),
    Hs(u64, u64),
    Ct(u64),
    Unknown,
}

/// Zustand von vor dem Call
#[derive(Clone, Debug, PartialEq, Eq)]
struct Saved {
    on: bool,
    brightness: Option<u64>,
    look: Look,
}

impl Saved {
    /// Aus der Antwort auf `GET /api/v1/<schlüssel>/`
    fn from_info(v: &Value) -> Saved {
        let st = &v["state"];
        let value = |k: &str| st[k]["value"].as_u64();
        let effect = v["effects"]["select"].as_str().filter(|e| !e.is_empty() && !e.starts_with('*'));
        let hs = match (value("hue"), value("sat")) {
            (Some(h), Some(s)) => Look::Hs(h, s),
            _ => Look::Unknown,
        };
        let look = match (st["colorMode"].as_str(), effect) {
            (Some("effect"), Some(e)) => Look::Effect(e.to_string()),
            (Some("ct"), _) => value("ct").map(Look::Ct).unwrap_or(hs),
            _ => hs,
        };
        Saved { on: st["on"]["value"].as_bool().unwrap_or(false), brightness: value("brightness"), look }
    }

    /// Anfragen (Pfad nach dem Schlüssel, Rumpf), die den Zustand
    /// wiederherstellen. Eine vorher ausgeschaltete Lampe wird zuletzt
    /// ausgeschaltet.
    fn requests(&self) -> Vec<(&'static str, Value)> {
        let mut state = json!({ "on": { "value": true } });
        if let Some(b) = self.brightness {
            state["brightness"] = json!({ "value": b, "duration": 0 });
        }
        let mut out = Vec::new();
        match &self.look {
            Look::Effect(name) => out.push(("effects", json!({ "select": name }))),
            Look::Hs(h, s) => {
                state["hue"] = json!({ "value": h });
                state["sat"] = json!({ "value": s });
            }
            Look::Ct(ct) => state["ct"] = json!({ "value": ct }),
            Look::Unknown => {}
        }
        out.push(("state", state));
        if !self.on {
            out.push(("state", json!({ "on": { "value": false } })));
        }
        out
    }
}

/// Funktion, die eine Anfrage schickt: Gerät, Methode, Pfad nach dem
/// Schlüssel, Rumpf. Liefert die Antwort (`Null` ohne Inhalt).
type SendFn<'a> = dyn FnMut(&Nanoleaf, &str, &str, Option<Value>) -> Result<Value, String> + 'a;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into()
}

fn read_body(mut r: ureq::http::Response<ureq::Body>) -> Result<Value, String> {
    let text = r.body_mut().read_to_string().map_err(|e| tr!("Antwort nicht lesbar: {e}", "Reply unreadable: {e}"))?;
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|_| tr!("Antwort ist kein JSON", "Reply is not JSON"))
}

fn send_real(n: &Nanoleaf, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("http://{}/api/v1/{}/{path}", n.host, n.key);
    let a = agent();
    let r = match (method, body) {
        ("PUT", Some(b)) => a.put(&url).send_json(&b),
        _ => a.get(&url).call(),
    };
    match r {
        Ok(r) => read_body(r).map_err(|e| format!("Nanoleaf {}: {e}", n.host)),
        Err(ureq::Error::StatusCode(401)) => Err(tr!("Nanoleaf {}: Schlüssel ungültig, neu koppeln", "Nanoleaf {}: key invalid, pair again", n.host)),
        Err(ureq::Error::StatusCode(c)) => Err(tr!("Nanoleaf {} meldet Fehler {c}", "Nanoleaf {} reports error {c}", n.host)),
        Err(e) => Err(tr!("Nanoleaf {} nicht erreichbar: {e}", "Nanoleaf {} unreachable: {e}", n.host)),
    }
}

/// Antwort auf `POST /api/v1/new` auswerten
fn parse_pair(v: &Value) -> Result<String, String> {
    v["auth_token"].as_str().filter(|k| !k.is_empty()).map(str::to_string).ok_or_else(|| tr!("Nanoleaf: unerwartete Antwort beim Koppeln", "Nanoleaf: unexpected reply while pairing"))
}

/// Mit dem Gerät koppeln (Einschaltknopf vorher 5–7 Sekunden halten). `host`
/// wie aus [`normalize`]. Liefert den Schlüssel.
pub fn pair(host: &str) -> Result<String, String> {
    match agent().post(&format!("http://{host}/api/v1/new")).send_empty() {
        Ok(r) => parse_pair(&read_body(r)?),
        Err(ureq::Error::StatusCode(401 | 403)) => Err(pair_hint()),
        Err(ureq::Error::StatusCode(c)) => Err(tr!("Nanoleaf {host} meldet Fehler {c}", "Nanoleaf {host} reports error {c}")),
        Err(e) => Err(tr!("Nanoleaf {host} nicht erreichbar: {e}", "Nanoleaf {host} unreachable: {e}")),
    }
}

/// Name des Geräts aus dem Bonjour-Namen („Canvas 5B3C._nanoleafapi._tcp.local.“)
fn instance_name(fullname: &str) -> String {
    fullname.strip_suffix(SERVICE).map(|s| s.trim_end_matches('.')).unwrap_or(fullname).replace("\\032", " ")
}

/// Geräte im eigenen Netz per Bonjour suchen (blockiert etwa drei Sekunden,
/// nicht aus einem Ausgabe-Thread aufrufen: `mdns-sd` startet einen eigenen).
/// Liefert Adresse mit Port und Namen, nach Adresse sortiert.
pub fn discover() -> Result<Vec<(String, String)>, String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let daemon = ServiceDaemon::new().map_err(|e| tr!("Nanoleaf-Suche nicht möglich: {e}", "Nanoleaf search not possible: {e}"))?;
    let rx = daemon.browse(SERVICE).map_err(|e| tr!("Nanoleaf-Suche nicht möglich: {e}", "Nanoleaf search not possible: {e}"))?;
    let deadline = Instant::now() + SCAN_WAIT;
    let mut found: Vec<(String, String)> = Vec::new();
    while let Ok(ev) = rx.recv_deadline(deadline) {
        if let ServiceEvent::ServiceResolved(info) = ev {
            let name = instance_name(info.get_fullname());
            let mut ips: Vec<_> = info.get_addresses_v4().into_iter().collect();
            ips.sort();
            if let Some(ip) = ips.first() {
                let host = format!("{ip}:{}", info.get_port());
                if !found.iter().any(|(h, _)| *h == host) {
                    found.push((host, name));
                }
            }
        }
    }
    let _ = daemon.shutdown();
    found.sort();
    Ok(found)
}

/// Zustand eines Geräts: was vorher war und ob gerade eine Farbe von MicLED
/// zu sehen ist
#[derive(Debug, Default)]
pub struct Session {
    cfg: Option<Nanoleaf>,
    /// `None` = noch kein Schnappschuss
    saved: Option<Saved>,
    /// Zuletzt sicher gezeigte Farbe
    shown: Option<Rgb>,
    /// Eine Farbe von MicLED ist womöglich zu sehen (auch nach einem Timeout)
    dirty: bool,
}

impl Device for Session {
    type Cfg = Nanoleaf;

    fn apply(&mut self, cfg: Option<&Nanoleaf>, scene: &Scene) -> Result<(), String> {
        self.apply_with(cfg, scene, &mut send_real)
    }

    fn restore(&mut self) -> Result<(), String> {
        let cfg = self.cfg.clone();
        self.apply(cfg.as_ref(), &Scene::Idle)
    }

    fn current(&self) -> Option<&Nanoleaf> {
        self.cfg.as_ref()
    }
}

impl Session {
    /// Wechselt die Adresse, wird das alte Gerät erst zurückgestellt. Scheitert
    /// das, bleibt der Zustand für einen weiteren Versuch erhalten. Ein neuer
    /// Schlüssel für dieselbe Adresse (neu gekoppelt) behält den Zustand.
    fn apply_with(&mut self, cfg: Option<&Nanoleaf>, scene: &Scene, send: &mut SendFn<'_>) -> Result<(), String> {
        if let Some(old) = self.cfg.clone().filter(|old| cfg.map(|c| &c.host) != Some(&old.host)) {
            self.idle(&old, send)?;
            *self = Session::default();
        }
        let Some(n) = cfg else { return Ok(()) };
        self.cfg = Some(n.clone());
        let Some(color) = scene_color(scene) else { return self.idle(n, send) };
        if self.shown == Some(color) {
            return Ok(());
        }
        if self.saved.is_none() {
            self.saved = Some(Saved::from_info(&send(n, "GET", "", None)?));
        }
        self.dirty = true;
        self.shown = None;
        send(n, "PUT", "state", Some(color_body(color)))?;
        self.shown = Some(color);
        Ok(())
    }

    fn idle(&mut self, n: &Nanoleaf, send: &mut SendFn<'_>) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        for (path, body) in self.saved.as_ref().map(Saved::requests).unwrap_or_default() {
            send(n, "PUT", path, Some(body))?;
        }
        self.dirty = false;
        self.shown = None;
        self.saved = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(host: &str) -> Nanoleaf {
        Nanoleaf { host: host.into(), key: "geheim".into() }
    }

    fn info(on: bool, bri: u64, mode: &str, effect: &str) -> Value {
        json!({
            "name": "Canvas 5B3C",
            "state": {
                "on": {"value": on},
                "brightness": {"value": bri, "max": 100, "min": 0},
                "hue": {"value": 120, "max": 360, "min": 0},
                "sat": {"value": 80, "max": 100, "min": 0},
                "ct": {"value": 2700, "max": 6500, "min": 1200},
                "colorMode": mode,
            },
            "effects": {"select": effect, "effectsList": ["Northern Lights"]},
        })
    }

    struct Fake {
        log: Vec<(String, String, Option<Value>)>,
        info: Value,
        fail: bool,
    }

    impl Fake {
        fn new(info: Value) -> Fake {
            Fake { log: vec![], info, fail: false }
        }

        fn send(&mut self) -> impl FnMut(&Nanoleaf, &str, &str, Option<Value>) -> Result<Value, String> + '_ {
            move |n, method, path, body| {
                if self.fail {
                    return Err(format!("Nanoleaf {} nicht erreichbar: timeout", n.host));
                }
                self.log.push((method.into(), path.into(), body));
                Ok(if method == "GET" { self.info.clone() } else { Value::Null })
            }
        }

        fn take(&mut self) -> Vec<(String, Option<Value>)> {
            self.log.drain(..).map(|(m, p, b)| (format!("{m} {p}").trim().to_string(), b)).collect()
        }
    }

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED };

    #[test]
    fn debug_hides_the_key_and_port_is_added() {
        assert!(!format!("{:?}", leaf("10.0.0.4:16021")).contains("geheim"));
        assert_eq!(normalize(" http://10.0.0.4/ ").as_deref(), Some("10.0.0.4:16021"));
        assert_eq!(normalize("10.0.0.4:1234").as_deref(), Some("10.0.0.4:1234"));
        assert_eq!(normalize("  "), None);
    }

    #[test]
    fn rgb_to_hsv() {
        assert_eq!(hsv(Rgb::RED), Some((0, 100, 100)));
        assert_eq!(hsv(Rgb::GREEN), Some((120, 100, 100)));
        assert_eq!(hsv(Rgb { r: 0x80, g: 0, b: 0xff }), Some((270, 100, 100)));
        assert_eq!(hsv(Rgb { r: 255, g: 255, b: 255 }), Some((0, 0, 100)));
        assert_eq!(hsv(Rgb { r: 128, g: 0, b: 0 }), Some((0, 100, 50)));
        assert_eq!(hsv(Rgb::OFF), None);
        assert_eq!(color_body(Rgb::OFF), json!({"on": {"value": false}}));
        assert_eq!(color_body(Rgb::GREEN)["brightness"], json!({"value": 100, "duration": 0}));
    }

    #[test]
    fn pairing_reply_and_names() {
        assert_eq!(parse_pair(&json!({"auth_token": "abc"})).unwrap(), "abc");
        assert!(parse_pair(&json!({})).is_err());
        assert_eq!(instance_name("Canvas 5B3C._nanoleafapi._tcp.local."), "Canvas 5B3C");
        assert_eq!(instance_name("Shapes\\0329A1F._nanoleafapi._tcp.local."), "Shapes 9A1F");
    }

    #[test]
    fn reads_the_state() {
        let s = Saved::from_info(&info(true, 60, "effect", "Northern Lights"));
        assert_eq!(s, Saved { on: true, brightness: Some(60), look: Look::Effect("Northern Lights".into()) });
        let s = Saved::from_info(&info(true, 60, "effect", "*Dynamic*"));
        assert_eq!(s.look, Look::Hs(120, 80), "ohne gespeicherten Effekt die Farbe");
        assert_eq!(Saved::from_info(&info(false, 30, "ct", "*Solid*")).look, Look::Ct(2700));
        assert_eq!(Saved::from_info(&info(true, 30, "hs", "*Solid*")).look, Look::Hs(120, 80));
        assert_eq!(Saved::from_info(&json!({})), Saved { on: false, brightness: None, look: Look::Unknown });
    }

    #[test]
    fn restore_requests() {
        let effect = Saved { on: true, brightness: Some(60), look: Look::Effect("Northern Lights".into()) };
        assert_eq!(
            effect.requests(),
            [
                ("effects", json!({"select": "Northern Lights"})),
                ("state", json!({"on": {"value": true}, "brightness": {"value": 60, "duration": 0}})),
            ]
        );
        let off_ct = Saved { on: false, brightness: Some(30), look: Look::Ct(2700) };
        assert_eq!(
            off_ct.requests(),
            [
                ("state", json!({"on": {"value": true}, "brightness": {"value": 30, "duration": 0}, "ct": {"value": 2700}})),
                ("state", json!({"on": {"value": false}})),
            ],
            "erst Weißton und Helligkeit, dann aus"
        );
        let hs = Saved { on: true, brightness: None, look: Look::Hs(10, 20) };
        assert_eq!(hs.requests(), [("state", json!({"on": {"value": true}, "hue": {"value": 10}, "sat": {"value": 20}}))]);
    }

    #[test]
    fn colored_and_effect_restored() {
        let mut f = Fake::new(info(true, 60, "effect", "Northern Lights"));
        let mut s = Session::default();
        let n = leaf("10.0.0.4:16021");
        s.apply_with(Some(&n), &LIVE, &mut f.send()).unwrap();
        let log = f.take();
        assert_eq!(log[0].0, "GET");
        assert_eq!(log[1], ("PUT state".into(), Some(color_body(Rgb::RED))));
        s.apply_with(Some(&n), &Scene::Timer { minutes: 4, color: Rgb::RED }, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "gleiche Farbe nicht neu senden");
        s.apply_with(Some(&n), &Scene::Idle, &mut f.send()).unwrap();
        let paths: Vec<String> = f.take().into_iter().map(|(p, _)| p).collect();
        assert_eq!(paths, ["PUT effects", "PUT state"]);
        s.apply_with(Some(&n), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn new_key_for_the_same_address_keeps_the_state() {
        let mut f = Fake::new(info(false, 30, "hs", "*Solid*"));
        let mut s = Session::default();
        s.apply_with(Some(&leaf("10.0.0.4:16021")), &LIVE, &mut f.send()).unwrap();
        f.take();
        let renewed = Nanoleaf { host: "10.0.0.4:16021".into(), key: "neu".into() };
        s.apply_with(Some(&renewed), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take().len(), 2, "Farbe von vorher, dann aus");
    }

    #[test]
    fn failures_are_reported_and_retried() {
        let mut f = Fake::new(info(true, 60, "hs", "*Solid*"));
        f.fail = true;
        let mut s = Session::default();
        let n = leaf("10.0.0.4:16021");
        let e = s.apply_with(Some(&n), &LIVE, &mut f.send()).unwrap_err();
        assert!(e.contains("10.0.0.4"), "{e}");
        f.fail = false;
        s.apply_with(Some(&n), &LIVE, &mut f.send()).unwrap();
        f.take();
        f.fail = true;
        assert!(s.apply_with(None, &Scene::Idle, &mut f.send()).is_err());
        assert_eq!(s.current(), Some(&n), "Zustand bleibt für den nächsten Versuch");
        f.fail = false;
        s.apply_with(None, &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take().len(), 1);
        assert_eq!(s.current(), None);
    }
}
