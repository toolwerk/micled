//! LED-Streifen und -Lampen mit WLED über die lokale JSON-API: Das Gerät zeigt
//! die Farbe der [`Scene`] und bekommt danach An/Aus, Helligkeit und je
//! Segment Farbe und Effekt von vorher zurück.
//!
//! HTTP, ohne Konto und ohne Schlüssel. Zustand mit `GET /json/state`,
//! Schalten mit `POST /json/state`. Gefunden werden die Geräte per Bonjour
//! (`_wled._tcp`).
//!
//! Lief vorher ein Preset, stellt MicLED dessen Aussehen je Segment zurück,
//! nicht das Preset selbst: Spätere Änderungen (etwa eine gedimmte
//! Helligkeit) bleiben so erhalten. Eine Playlist startet neu. Live-Daten
//! einer anderen Steuerung überschreibt MicLED für die Dauer des Calls
//! (`lor`) und gibt sie danach wieder frei.

use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::color::Rgb;
use crate::display::Scene;
use crate::output::{bonjour, scene_color, Device, Keyed};

const TIMEOUT: Duration = Duration::from_secs(3);
const SERVICE: &str = "_wled._tcp.local.";

/// Schlüssel eines Segments, die [`color_body`] ändert und die deshalb
/// gemerkt werden
const SEG_KEYS: [&str; 6] = ["id", "on", "bri", "col", "fx", "frz"];

/// WLED-Gerät
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wled {
    /// Adresse, Port nur, wenn angegeben, siehe [`normalize`]
    pub host: String,
}

impl Keyed for Wled {
    fn address(&self) -> &str {
        &self.host
    }
}

/// Adresse aus den Einstellungen bereinigen wie [`crate::pixoo::normalize`],
/// ein angegebener Port bleibt, außer dem Standard-Port 80 (sonst zählte
/// dasselbe Gerät doppelt). Leer = aus.
pub fn normalize(host: &str) -> Option<String> {
    let h = crate::pixoo::normalize(host)?;
    match h.strip_suffix(":80") {
        Some("") => None,
        Some(bare) => Some(bare.to_string()),
        None => Some(h),
    }
}

/// Befehl für die Farbe `c` auf den Segmenten `ids` (`OFF` schaltet aus),
/// einfarbig, ohne Überblendung und vor Live-Daten anderer Steuerungen.
/// `tt` statt `transition`, weil `transition` im Gerät gespeichert bliebe.
fn color_body(c: Rgb, ids: &[u64]) -> Value {
    if c == Rgb::OFF {
        return json!({ "on": false, "tt": 0, "lor": 1 });
    }
    let seg: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "id": id, "on": true, "bri": 255, "col": [[c.r, c.g, c.b, 0]], "fx": 0, "frz": false }))
        .collect();
    json!({ "on": true, "bri": 255, "tt": 0, "lor": 1, "seg": seg })
}

/// Zustand von vor dem Call
#[derive(Clone, Debug, PartialEq)]
struct Saved {
    on: bool,
    bri: Option<u64>,
    /// Vorrang vor Live-Daten (0 = keiner)
    lor: u64,
    /// Laufende Playlist
    playlist: Option<i64>,
    /// Je Segment genau die Schlüssel aus [`SEG_KEYS`], die es gab
    segs: Vec<Map<String, Value>>,
}

impl Saved {
    /// Aus der Antwort auf `GET /json/state`. Segmente ohne `id` fallen weg.
    fn from_state(v: &Value) -> Saved {
        let segs = v["seg"]
            .as_array()
            .map(|segs| {
                segs.iter()
                    .filter(|s| s["id"].is_u64())
                    .map(|s| SEG_KEYS.iter().filter_map(|k| s.get(*k).map(|v| (k.to_string(), v.clone()))).collect())
                    .collect()
            })
            .unwrap_or_default();
        Saved {
            on: v["on"].as_bool().unwrap_or(false),
            bri: v["bri"].as_u64(),
            lor: v["lor"].as_u64().unwrap_or(0),
            playlist: v["pl"].as_i64().filter(|p| *p >= 0),
            segs,
        }
    }

    /// Segmente, die die Farbe zeigen; ohne Segmente in der Antwort nur 0
    fn ids(&self) -> Vec<u64> {
        let ids: Vec<u64> = self.segs.iter().filter_map(|s| s["id"].as_u64()).collect();
        if ids.is_empty() {
            vec![0]
        } else {
            ids
        }
    }

    /// Rümpfe für `POST /json/state`, die den Zustand wiederherstellen. Eine
    /// Playlist startet neu, ein vorher ausgeschaltetes Gerät wird danach
    /// ausgeschaltet.
    fn requests(&self) -> Vec<Value> {
        if let Some(pl) = self.playlist {
            let mut out = vec![json!({ "ps": pl, "lor": self.lor, "tt": 0 })];
            if !self.on {
                out.push(json!({ "on": false }));
            }
            return out;
        }
        let mut body = json!({ "on": self.on, "lor": self.lor, "tt": 0 });
        if let Some(b) = self.bri {
            body["bri"] = json!(b);
        }
        if !self.segs.is_empty() {
            body["seg"] = json!(self.segs);
        }
        vec![body]
    }
}

/// Funktion, die eine Anfrage schickt: ohne Rumpf `GET /json/state` (liefert
/// den Zustand), mit Rumpf `POST /json/state`
type SendFn<'a> = dyn FnMut(&Wled, Option<Value>) -> Result<Value, String> + 'a;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into()
}

/// Zustand aus der Antwort auf `GET`; alles außer einem JSON-Objekt ist ein Fehler
fn parse_state(text: &str) -> Result<Value, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) if v.is_object() => Ok(v),
        _ => Err("Antwort ist kein JSON".into()),
    }
}

fn send_real(w: &Wled, body: Option<Value>) -> Result<Value, String> {
    let url = format!("http://{}/json/state", w.host);
    let a = agent();
    let get = body.is_none();
    let r = match body {
        Some(b) => a.post(&url).send_json(&b),
        None => a.get(&url).call(),
    };
    match r {
        // die Antwort auf POST (`{"success":true}`) ist kein Zustand
        Ok(_) if !get => Ok(Value::Null),
        Ok(mut r) => {
            let text = r.body_mut().read_to_string().map_err(|e| format!("WLED {}: Antwort nicht lesbar: {e}", w.host))?;
            parse_state(&text).map_err(|e| format!("WLED {}: {e}", w.host))
        }
        Err(ureq::Error::StatusCode(c)) => Err(format!("WLED {} meldet Fehler {c}", w.host)),
        Err(e) => Err(format!("WLED {} nicht erreichbar: {e}", w.host)),
    }
}

/// Adresse zum Eintragen: bei Port 80 ohne Port
fn found_host(ip: std::net::Ipv4Addr, port: u16) -> String {
    if port == 80 {
        ip.to_string()
    } else {
        format!("{ip}:{port}")
    }
}

/// Geräte im eigenen Netz per Bonjour suchen (blockiert etwa drei Sekunden,
/// nicht aus einem Ausgabe-Thread aufrufen: `mdns-sd` startet einen eigenen).
/// Liefert Adresse und Namen, nach Adresse sortiert.
pub fn discover() -> Result<Vec<(String, String)>, String> {
    let mut found: Vec<(String, String)> =
        bonjour(SERVICE, "WLED")?.into_iter().map(|(ip, port, name)| (found_host(ip, port), name)).collect();
    found.sort();
    Ok(found)
}

/// Zustand eines Geräts: was vorher war und ob gerade eine Farbe von MicLED
/// zu sehen ist
#[derive(Debug, Default)]
pub struct Session {
    cfg: Option<Wled>,
    /// `None` = noch kein Schnappschuss
    saved: Option<Saved>,
    /// Zuletzt sicher gezeigte Farbe
    shown: Option<Rgb>,
    /// Eine Farbe von MicLED ist womöglich zu sehen (auch nach einem Timeout)
    dirty: bool,
}

impl Device for Session {
    type Cfg = Wled;

    fn apply(&mut self, cfg: Option<&Wled>, scene: &Scene) -> Result<(), String> {
        self.apply_with(cfg, scene, &mut send_real)
    }

    fn restore(&mut self) -> Result<(), String> {
        let cfg = self.cfg.clone();
        self.apply(cfg.as_ref(), &Scene::Idle)
    }

    fn current(&self) -> Option<&Wled> {
        self.cfg.as_ref()
    }
}

impl Session {
    /// Wechselt die Adresse, wird das alte Gerät erst zurückgestellt. Scheitert
    /// das, bleibt der Zustand für einen weiteren Versuch erhalten.
    fn apply_with(&mut self, cfg: Option<&Wled>, scene: &Scene, send: &mut SendFn<'_>) -> Result<(), String> {
        if let Some(old) = self.cfg.clone().filter(|old| cfg != Some(old)) {
            self.idle(&old, send)?;
            *self = Session::default();
        }
        let Some(w) = cfg else { return Ok(()) };
        self.cfg = Some(w.clone());
        let Some(color) = scene_color(scene) else { return self.idle(w, send) };
        if self.shown == Some(color) {
            return Ok(());
        }
        if self.saved.is_none() {
            self.saved = Some(Saved::from_state(&send(w, None)?));
        }
        let ids = self.saved.as_ref().map(Saved::ids).unwrap_or_default();
        self.dirty = true;
        self.shown = None;
        send(w, Some(color_body(color, &ids)))?;
        self.shown = Some(color);
        Ok(())
    }

    fn idle(&mut self, w: &Wled, send: &mut SendFn<'_>) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        for body in self.saved.as_ref().map(Saved::requests).unwrap_or_default() {
            send(w, Some(body))?;
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

    fn wled(host: &str) -> Wled {
        Wled { host: host.into() }
    }

    /// Antwort auf `GET /json/state` mit einem Effekt auf Segment 0
    fn state(on: bool, bri: u64, pl: i64) -> Value {
        json!({
            "on": on, "bri": bri, "transition": 7, "ps": 3, "pl": pl, "lor": 0,
            "nl": {"on": false, "dur": 60},
            "seg": [{
                "id": 0, "start": 0, "stop": 60, "on": true, "bri": 200, "frz": false,
                "col": [[255, 160, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
                "fx": 9, "sx": 128, "ix": 128, "pal": 6, "sel": true,
            }],
        })
    }

    struct Fake {
        log: Vec<Option<Value>>,
        state: Value,
        fail: bool,
    }

    impl Fake {
        fn new(state: Value) -> Fake {
            Fake { log: vec![], state, fail: false }
        }

        fn send(&mut self) -> impl FnMut(&Wled, Option<Value>) -> Result<Value, String> + '_ {
            move |w, body| {
                if self.fail {
                    return Err(format!("WLED {} nicht erreichbar: timeout", w.host));
                }
                let get = body.is_none();
                self.log.push(body);
                Ok(if get { self.state.clone() } else { Value::Null })
            }
        }

        fn take(&mut self) -> Vec<Option<Value>> {
            std::mem::take(&mut self.log)
        }
    }

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED };

    #[test]
    fn addresses_keep_a_given_port() {
        assert_eq!(normalize(" http://10.0.0.5/ ").as_deref(), Some("10.0.0.5"));
        assert_eq!(normalize("10.0.0.5:8080").as_deref(), Some("10.0.0.5:8080"));
        assert_eq!(normalize("http://10.0.0.5:80/").as_deref(), Some("10.0.0.5"), "Port 80 ist derselbe Eintrag");
        assert_eq!(normalize("10.0.0.5:8000").as_deref(), Some("10.0.0.5:8000"));
        assert_eq!(normalize("  "), None);
        assert_eq!(found_host([10, 0, 0, 5].into(), 80), "10.0.0.5");
        assert_eq!(found_host([10, 0, 0, 5].into(), 8080), "10.0.0.5:8080");
        assert_eq!(crate::output::instance_name("Schreibtisch._wled._tcp.local.", SERVICE), "Schreibtisch");
    }

    #[test]
    fn state_must_be_json() {
        assert!(parse_state(r#"{"on":true}"#).is_ok());
        assert!(parse_state("<html>").is_err());
        assert!(parse_state("").is_err());
        assert!(parse_state("[1]").is_err());
    }

    #[test]
    fn color_on_every_segment() {
        assert_eq!(color_body(Rgb::OFF, &[0]), json!({"on": false, "tt": 0, "lor": 1}));
        let seg = |id: u64| json!({"id": id, "on": true, "bri": 255, "col": [[0, 255, 0, 0]], "fx": 0, "frz": false});
        assert_eq!(
            color_body(Rgb::GREEN, &[0, 2]),
            json!({"on": true, "bri": 255, "tt": 0, "lor": 1, "seg": [seg(0), seg(2)]})
        );
    }

    #[test]
    fn remembers_exactly_the_changed_keys() {
        let s = Saved::from_state(&state(true, 128, -1));
        assert_eq!((s.on, s.bri, s.lor, s.playlist), (true, Some(128), 0, None));
        assert_eq!(
            Value::Object(s.segs[0].clone()),
            json!({"id": 0, "on": true, "bri": 200, "col": [[255, 160, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]], "fx": 9, "frz": false})
        );
        assert_eq!(
            s.requests(),
            [json!({"on": true, "bri": 128, "lor": 0, "tt": 0, "seg": [s.segs[0].clone()]})],
            "Preset 3 wird nicht neu geladen, sondern sein Aussehen zurückgestellt"
        );
        let partial = Saved::from_state(&json!({"on": true, "seg": [{"id": 1, "fx": 2}, {"fx": 3}]}));
        assert_eq!(partial.segs.len(), 1, "Segment ohne id fällt weg");
        assert_eq!(Value::Object(partial.segs[0].clone()), json!({"id": 1, "fx": 2}));
        assert_eq!(partial.ids(), [1]);
    }

    #[test]
    fn empty_reply_means_off_without_segments() {
        let s = Saved::from_state(&json!({}));
        assert_eq!(s, Saved { on: false, bri: None, lor: 0, playlist: None, segs: vec![] });
        assert_eq!(s.ids(), [0]);
        assert_eq!(s.requests(), [json!({"on": false, "lor": 0, "tt": 0})]);
    }

    #[test]
    fn playlist_restarts_and_live_override_is_restored() {
        let mut v = state(true, 128, 5);
        v["lor"] = json!(2);
        let s = Saved::from_state(&v);
        assert_eq!(s.requests(), [json!({"ps": 5, "lor": 2, "tt": 0})]);
        let off = Saved::from_state(&state(false, 128, 5));
        assert_eq!(off.requests(), [json!({"ps": 5, "lor": 0, "tt": 0}), json!({"on": false})], "erst Playlist, dann aus");
    }

    #[test]
    fn switched_off_device_is_switched_off_again() {
        let s = Saved::from_state(&state(false, 90, -1));
        let body = &s.requests()[0];
        assert_eq!(body["on"], json!(false));
        assert_eq!(body["bri"], json!(90));
        assert_eq!(body["seg"][0]["fx"], json!(9));
    }

    #[test]
    fn colored_and_restored() {
        let mut f = Fake::new(state(true, 128, -1));
        let mut s = Session::default();
        let w = wled("10.0.0.5");
        s.apply_with(Some(&w), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), [None, Some(color_body(Rgb::RED, &[0]))]);
        s.apply_with(Some(&w), &Scene::Timer { minutes: 4, color: Rgb::RED }, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "gleiche Farbe nicht neu senden");
        s.apply_with(Some(&w), &Scene::Focus { color: Rgb::GREEN }, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Some(color_body(Rgb::GREEN, &[0]))], "kein zweiter Schnappschuss");
        s.apply_with(Some(&w), &Scene::Idle, &mut f.send()).unwrap();
        let log = f.take();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].as_ref().unwrap()["seg"][0]["fx"], json!(9));
        s.apply_with(Some(&w), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn two_segments_get_the_color() {
        let mut v = state(true, 128, -1);
        let mut second = v["seg"][0].clone();
        second["id"] = json!(1);
        second["fx"] = json!(0);
        v["seg"].as_array_mut().unwrap().push(second);
        let mut f = Fake::new(v);
        let mut s = Session::default();
        let w = wled("10.0.0.5");
        s.apply_with(Some(&w), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take()[1], Some(color_body(Rgb::RED, &[0, 1])));
        s.apply_with(Some(&w), &Scene::Idle, &mut f.send()).unwrap();
        let restored = f.take().remove(0).unwrap();
        assert_eq!(restored["seg"][0]["fx"], json!(9));
        assert_eq!(restored["seg"][1]["fx"], json!(0));
    }

    #[test]
    fn address_change_restores_the_old_device_first() {
        let mut f = Fake::new(state(true, 128, -1));
        let mut s = Session::default();
        s.apply_with(Some(&wled("10.0.0.5")), &LIVE, &mut f.send()).unwrap();
        f.take();
        s.apply_with(Some(&wled("10.0.0.6")), &LIVE, &mut f.send()).unwrap();
        let log = f.take();
        assert_eq!(log.len(), 3, "altes zurück, neues lesen, neues färben");
        assert_eq!(log[0].as_ref().unwrap()["seg"][0]["fx"], json!(9));
        assert_eq!(log[1], None);
        assert_eq!(s.current(), Some(&wled("10.0.0.6")));
    }

    #[test]
    fn failures_are_reported_and_retried() {
        let mut f = Fake::new(state(true, 128, -1));
        f.fail = true;
        let mut s = Session::default();
        let w = wled("10.0.0.5");
        let e = s.apply_with(Some(&w), &LIVE, &mut f.send()).unwrap_err();
        assert!(e.contains("10.0.0.5"), "{e}");
        f.fail = false;
        s.apply_with(Some(&w), &LIVE, &mut f.send()).unwrap();
        f.take();
        f.fail = true;
        assert!(s.apply_with(None, &Scene::Idle, &mut f.send()).is_err());
        assert_eq!(s.current(), Some(&w), "Zustand bleibt für den nächsten Versuch");
        f.fail = false;
        s.apply_with(None, &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take().len(), 1);
        assert_eq!(s.current(), None);
    }
}
