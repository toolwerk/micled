//! Philips Hue über die Hue Bridge im eigenen Netz (lokale CLIP-v2-API): Eine
//! Lampe oder ein Raum zeigt die Farbe der [`Scene`] und bekommt danach
//! An/Aus, Farbe und Helligkeit von vorher zurück.
//!
//! Koppeln: Knopf an der Bridge drücken, dann `POST /api`. Schalten:
//! `PUT /clip/v2/resource/light/<id>` bzw. `grouped_light/<id>` für einen Raum,
//! Farben als xy-Werte. Die Verbindung läuft über [`crate::hue_net`].

use std::fmt;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::color::Rgb;
use crate::display::Scene;
use crate::hue_net;
use crate::output::Device;

/// Abstand zwischen zwei Befehlen an Lampen (die Bridge verarbeitet etwa
/// zehn pro Sekunde)
const SPACING: Duration = Duration::from_millis(100);
/// Abstand zwischen zwei Befehlen an einen Raum (`grouped_light`, etwa einer
/// pro Sekunde)
const GROUP_SPACING: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Light,
    Room,
}

/// Lampe oder Raum auf der Bridge
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub kind: Kind,
    pub id: String,
}

/// Zugang zur Bridge und gewähltes Ziel
#[derive(Clone, PartialEq, Eq)]
pub struct Hue {
    pub host: String,
    /// App-Schlüssel vom Koppeln
    pub key: String,
    /// SHA-256 des Zertifikats vom Koppeln (Hex)
    pub fingerprint: String,
    pub target: Target,
}

impl fmt::Debug for Hue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hue")
            .field("host", &self.host)
            .field("key", &"…")
            .field("fingerprint", &self.fingerprint)
            .field("target", &self.target)
            .finish()
    }
}

fn srgb_to_linear(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v > 0.04045 {
        ((v + 0.055) / 1.055).powf(2.4)
    } else {
        v / 12.92
    }
}

/// Farbe als xy-Wert und Helligkeit in Prozent. `OFF` ergibt `None`.
pub fn xy(c: Rgb) -> Option<(f64, f64, f64)> {
    if c == Rgb::OFF {
        return None;
    }
    let (r, g, b) = (srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b));
    // Wide RGB D65
    let x = 0.664511 * r + 0.154324 * g + 0.162028 * b;
    let y = 0.283881 * r + 0.668433 * g + 0.047685 * b;
    let z = 0.000088 * r + 0.072310 * g + 0.986039 * b;
    let sum = x + y + z;
    let brightness = c.r.max(c.g).max(c.b) as f64 / 255.0 * 100.0;
    Some((x / sum, y / sum, brightness))
}

/// Farbe einer Szene, `None` bei `Idle`
fn scene_color(scene: &Scene) -> Option<Rgb> {
    match scene {
        Scene::Idle => None,
        Scene::Live { color, .. } | Scene::Timer { color, .. } | Scene::Focus { color } => Some(*color),
    }
}

/// Befehl, der eine Lampe in der Farbe `c` zeigt (`OFF` schaltet aus)
fn color_body(c: Rgb) -> Value {
    match xy(c) {
        None => json!({ "on": { "on": false } }),
        Some((x, y, bri)) => json!({
            "on": { "on": true },
            "dimming": { "brightness": bri },
            "color": { "xy": { "x": x, "y": y } },
        }),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum SavedColor {
    Xy(f64, f64),
    Mirek(u64),
    Unknown,
}

/// Zustand einer Lampe von vor dem Call
#[derive(Clone, Debug, PartialEq)]
struct Saved {
    id: String,
    on: bool,
    brightness: Option<f64>,
    color: SavedColor,
    /// Lampe kann Farben (hat `color`); sonst nur An/Aus, Helligkeit, Weißton
    supports_color: bool,
    /// Lampe lässt sich dimmen (hat `dimming`); eine Steckdose etwa nicht
    supports_dimming: bool,
}

impl Saved {
    /// Aus einem Eintrag von `GET /clip/v2/resource/light`. Fehlende Felder
    /// gelten als nicht gesetzt.
    fn from_light(l: &Value) -> Option<Saved> {
        let id = l["id"].as_str()?.to_string();
        let ct = &l["color_temperature"];
        let color = match (ct["mirek_valid"].as_bool(), ct["mirek"].as_u64()) {
            (Some(true), Some(m)) => SavedColor::Mirek(m),
            _ => match (l["color"]["xy"]["x"].as_f64(), l["color"]["xy"]["y"].as_f64()) {
                (Some(x), Some(y)) => SavedColor::Xy(x, y),
                _ => SavedColor::Unknown,
            },
        };
        Some(Saved {
            id,
            on: l["on"]["on"].as_bool().unwrap_or(false),
            brightness: l["dimming"]["brightness"].as_f64(),
            color,
            supports_color: l["color"].is_object(),
            supports_dimming: l["dimming"].is_object(),
        })
    }

    /// Befehle, die den gemerkten Zustand wiederherstellen. Eine vorher
    /// ausgeschaltete Lampe bekommt erst Farbe und Helligkeit (die Lampe nimmt
    /// sie nur eingeschaltet an) und wird dann ausgeschaltet, damit sie beim
    /// nächsten Einschalten nicht in der Call-Farbe leuchtet.
    fn bodies(&self) -> Vec<Value> {
        let off = json!({ "on": { "on": false } });
        let lit = self.lit_body();
        match (self.on, lit) {
            (true, Some(b)) => vec![b],
            (true, None) => vec![json!({ "on": { "on": true } })],
            // ohne Überblendung, sonst geht die Lampe in einer Mischfarbe aus
            (false, Some(mut b)) => {
                b["dynamics"] = json!({ "duration": 0 });
                vec![b, off]
            }
            (false, None) => vec![off],
        }
    }

    /// Eingeschaltet mit gemerkter Farbe und Helligkeit, `None` wenn nichts bekannt ist
    fn lit_body(&self) -> Option<Value> {
        if self.brightness.is_none() && self.color == SavedColor::Unknown {
            return None;
        }
        let mut b = json!({ "on": { "on": true } });
        if let Some(bri) = self.brightness {
            b["dimming"] = json!({ "brightness": bri });
        }
        match self.color {
            SavedColor::Xy(x, y) => b["color"] = json!({ "xy": { "x": x, "y": y } }),
            SavedColor::Mirek(m) => b["color_temperature"] = json!({ "mirek": m }),
            SavedColor::Unknown => {}
        }
        Some(b)
    }

    /// Farbbefehl passend zur Lampe: ohne `color` bzw. `dimming`, wenn sie
    /// das nicht kann
    fn adapt(&self, body: &Value) -> Value {
        let mut b = body.clone();
        if let Some(o) = b.as_object_mut() {
            if !self.supports_color {
                o.remove("color");
            }
            if !self.supports_dimming {
                o.remove("dimming");
            }
        }
        b
    }
}

/// `data` einer CLIP-v2-Antwort
fn data(v: &Value) -> &[Value] {
    v["data"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// IDs der Verweise eines Raums (`services` bzw. `children`) mit diesem `rtype`
fn refs<'a>(room: &'a Value, field: &str, rtype: &str) -> Vec<&'a str> {
    room[field]
        .as_array()
        .map(|a| a.iter().filter(|r| r["rtype"] == rtype).filter_map(|r| r["rid"].as_str()).collect())
        .unwrap_or_default()
}

/// Funktion, die eine Anfrage an die Bridge schickt: Zugang, Methode, Pfad, Rumpf
type SendFn<'a> = dyn FnMut(&Hue, &str, &str, Option<Value>) -> Result<Value, String> + 'a;

fn send_real(h: &Hue, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    hue_net::request(&h.host, Some(&h.fingerprint), method, path, Some(&h.key), body).map(|(v, _)| v)
}

/// Zustand einer Lampe oder eines Raums: was vorher war und ob gerade eine
/// Farbe von MicLED zu sehen ist
#[derive(Debug, Default)]
pub struct Session {
    cfg: Option<Hue>,
    /// Lampen mit ihrem Zustand von vorher, `None` = noch kein Schnappschuss
    saved: Option<Vec<Saved>>,
    /// `grouped_light` des Raums, darüber wird die Farbe gesetzt
    grouped: Option<String>,
    /// Zuletzt sicher gezeigte Farbe (die Restminuten des Timers zählen nicht)
    shown: Option<Rgb>,
    /// Eine Farbe von MicLED ist womöglich zu sehen (auch nach einem Timeout)
    dirty: bool,
    /// Zeit des letzten Befehls an eine Lampe bzw. einen Raum (für die Abstände)
    last_put: Option<Instant>,
    last_group: Option<Instant>,
    /// Lampen früherer Ziele, die beim Wechsel nicht zurückgestellt werden
    /// konnten; werden bei jedem Durchgang erneut versucht
    orphans: Vec<Orphan>,
}

/// Nicht zurückgestellte Lampen eines früheren Ziels
#[derive(Debug)]
struct Orphan {
    hue: Hue,
    saved: Vec<Saved>,
    /// Bisherige Versuche
    tries: u32,
}

/// So oft wird ein früheres Ziel versucht, danach aufgegeben (alte Adresse
/// nicht mehr erreichbar)
const MAX_ORPHAN_TRIES: u32 = 5;

impl Device for Session {
    type Cfg = Hue;

    fn apply(&mut self, cfg: Option<&Hue>, scene: &Scene) -> Result<(), String> {
        let (mut last, mut last_group) = (self.last_put, self.last_group);
        let r = self.apply_with(cfg, scene, &mut |h, method, path, body| {
            if method == "PUT" {
                let (slot, spacing) = if path.contains("/grouped_light/") {
                    (&mut last_group, GROUP_SPACING)
                } else {
                    (&mut last, SPACING)
                };
                if let Some(wait) = slot.and_then(|t| spacing.checked_sub(t.elapsed())) {
                    std::thread::sleep(wait);
                }
                *slot = Some(Instant::now());
            }
            send_real(h, method, path, body)
        });
        self.last_put = last;
        self.last_group = last_group;
        r
    }

    fn restore(&mut self) -> Result<(), String> {
        let cfg = self.cfg.clone();
        self.apply(cfg.as_ref(), &Scene::Idle)
    }

    fn current(&self) -> Option<&Hue> {
        self.cfg.as_ref()
    }
}

impl Session {
    fn apply_with(&mut self, cfg: Option<&Hue>, scene: &Scene, send: &mut SendFn<'_>) -> Result<(), String> {
        if self.cfg.is_some() && self.cfg.as_ref() != cfg {
            let mut orphans = std::mem::take(&mut self.orphans);
            if let Some(old) = self.cfg.clone() {
                if let Err(e) = self.idle(&old, send) {
                    log::warn!("alte Hue-Lampe nicht zurückgestellt: {e}");
                    if let Some(saved) = self.saved.take() {
                        orphans.push(Orphan { hue: old, saved, tries: 1 });
                    }
                }
            }
            *self = Session { last_put: self.last_put, last_group: self.last_group, orphans, ..Session::default() };
        }
        // erst das aktuelle Ziel, damit eine nicht erreichbare alte Adresse es nicht aufhält
        self.apply_target(cfg, scene, send)?;
        match self.retry_orphans(send) {
            Some(e) => Err(format!("Frühere Hue-Lampe nicht zurückgestellt: {e}")),
            None => Ok(()),
        }
    }

    fn apply_target(&mut self, cfg: Option<&Hue>, scene: &Scene, send: &mut SendFn<'_>) -> Result<(), String> {
        let Some(h) = cfg else { return Ok(()) };
        self.cfg = Some(h.clone());
        let Some(color) = scene_color(scene) else { return self.idle(h, send) };
        if self.shown == Some(color) {
            return Ok(());
        }
        if self.saved.is_none() {
            self.snapshot(h, send)?;
        }
        self.dirty = true;
        self.shown = None;
        let body = color_body(color);
        match (&self.grouped, h.target.kind) {
            (Some(gid), Kind::Room) => {
                send(h, "PUT", &format!("/clip/v2/resource/grouped_light/{gid}"), Some(body))?;
            }
            _ => {
                let lights: Vec<(String, Value)> = self.saved.iter().flatten().map(|s| (s.id.clone(), s.adapt(&body))).collect();
                for (id, b) in lights {
                    send(h, "PUT", &format!("/clip/v2/resource/light/{id}"), Some(b))?;
                }
            }
        }
        self.shown = Some(color);
        Ok(())
    }

    /// Lampen des Ziels auflösen und ihren Zustand merken
    fn snapshot(&mut self, h: &Hue, send: &mut SendFn<'_>) -> Result<(), String> {
        let (lights, grouped) = match h.target.kind {
            Kind::Light => {
                let v = send(h, "GET", &format!("/clip/v2/resource/light/{}", h.target.id), None)?;
                (data(&v).iter().filter_map(Saved::from_light).collect::<Vec<_>>(), None)
            }
            Kind::Room => {
                let v = send(h, "GET", &format!("/clip/v2/resource/room/{}", h.target.id), None)?;
                let room = data(&v).first().ok_or("Raum auf der Hue-Bridge nicht gefunden")?;
                let grouped = refs(room, "services", "grouped_light").first().map(|s| s.to_string());
                let devices = refs(room, "children", "device");
                let all = send(h, "GET", "/clip/v2/resource/light", None)?;
                let lights = data(&all)
                    .iter()
                    .filter(|l| l["owner"]["rid"].as_str().is_some_and(|rid| devices.contains(&rid)))
                    .filter_map(Saved::from_light)
                    .collect();
                (lights, grouped)
            }
        };
        let mut lights = lights;
        // Lampen, die noch vom früheren Ziel in der Call-Farbe leuchten:
        // deren Zustand von vorher gilt, und das alte Ziel lässt sie in Ruhe
        for o in self.orphans.iter_mut().filter(|o| o.hue.host == h.host) {
            for l in &mut lights {
                if let Some(i) = o.saved.iter().position(|s| s.id == l.id) {
                    *l = o.saved.remove(i);
                }
            }
        }
        self.orphans.retain(|o| !o.saved.is_empty());
        if lights.is_empty() {
            return Err(match h.target.kind {
                Kind::Light => "Lampe auf der Hue-Bridge nicht gefunden".into(),
                Kind::Room => "Raum hat keine Hue-Lampen".into(),
            });
        }
        self.saved = Some(lights);
        self.grouped = grouped;
        Ok(())
    }

    fn idle(&mut self, h: &Hue, send: &mut SendFn<'_>) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let saved = self.saved.clone().unwrap_or_default();
        restore_all(h, &saved, send)?;
        self.dirty = false;
        self.shown = None;
        self.saved = None;
        self.grouped = None;
        Ok(())
    }
}

impl Session {
    /// Lampen früherer Ziele zurückstellen, falls das beim Wechsel scheiterte.
    /// Liefert den letzten Fehler, solange noch eines offen ist.
    fn retry_orphans(&mut self, send: &mut SendFn<'_>) -> Option<String> {
        let mut error = None;
        let mut open = Vec::new();
        for mut o in std::mem::take(&mut self.orphans) {
            match restore_all(&o.hue, &o.saved, send) {
                Ok(()) => {}
                Err(e) if o.tries + 1 >= MAX_ORPHAN_TRIES => {
                    log::warn!("frühere Hue-Lampe aufgegeben: {e}");
                }
                Err(e) => {
                    o.tries += 1;
                    error = Some(e);
                    open.push(o);
                }
            }
        }
        self.orphans = open;
        error
    }
}

/// Jede Lampe auf ihren gemerkten Zustand
fn restore_all(h: &Hue, saved: &[Saved], send: &mut SendFn<'_>) -> Result<(), String> {
    for s in saved {
        for b in s.bodies() {
            send(h, "PUT", &format!("/clip/v2/resource/light/{}", s.id), Some(b))?;
        }
    }
    Ok(())
}

/// Antwort auf `POST /api` auswerten: Schlüssel oder Fehlermeldung
fn parse_pair(v: &Value) -> Result<String, String> {
    let first = v.as_array().and_then(|a| a.first()).ok_or("Hue-Bridge: unerwartete Antwort beim Koppeln")?;
    if let Some(key) = first["success"]["username"].as_str() {
        return Ok(key.to_string());
    }
    match first["error"]["type"].as_i64() {
        Some(101) => Err("Knopf an der Bridge drücken, dann innerhalb von 30 Sekunden erneut koppeln".into()),
        _ => Err(match first["error"]["description"].as_str() {
            Some(d) => format!("Hue-Bridge meldet: {d}"),
            None => "Hue-Bridge: unerwartete Antwort beim Koppeln".into(),
        }),
    }
}

/// Mit der Bridge koppeln (Knopf vorher drücken). Liefert App-Schlüssel und
/// Fingerabdruck des Zertifikats.
pub fn pair(host: &str) -> Result<(String, String), String> {
    let (v, fingerprint) = hue_net::request(host, None, "POST", "/api", None, Some(json!({ "devicetype": "micled#mac" })))?;
    Ok((parse_pair(&v)?, fingerprint))
}

/// Räume und Lampen aus den Antworten von `GET …/room` und `GET …/light`:
/// erst die Räume, dann die Lampen, jeweils nach Namen sortiert
fn parse_targets(rooms: &Value, lights: &Value) -> Vec<(Target, String)> {
    let list = |v: &Value, kind: Kind| {
        let mut items: Vec<(Target, String)> = data(v)
            .iter()
            .filter_map(|e| {
                let id = e["id"].as_str()?.to_string();
                let name = e["metadata"]["name"].as_str().unwrap_or(&id).to_string();
                Some((Target { kind, id }, name))
            })
            .collect();
        items.sort_by_key(|(_, n)| n.to_lowercase());
        items
    };
    let mut out = list(rooms, Kind::Room);
    out.extend(list(lights, Kind::Light));
    out
}

/// Räume und Lampen der Bridge zur Auswahl (Zugang aus `h`, das Ziel zählt nicht)
pub fn targets(h: &Hue) -> Result<Vec<(Target, String)>, String> {
    let rooms = send_real(h, "GET", "/clip/v2/resource/room", None)?;
    let lights = send_real(h, "GET", "/clip/v2/resource/light", None)?;
    Ok(parse_targets(&rooms, &lights))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.005
    }

    #[test]
    fn rgb_to_xy() {
        let (x, y, bri) = xy(Rgb::RED).unwrap();
        assert!(near(x, 0.701) && near(y, 0.299), "Rot: {x} {y}");
        assert!(near(bri, 100.0));
        let (x, y, _) = xy(Rgb::GREEN).unwrap();
        assert!(near(x, 0.172) && near(y, 0.747), "Grün: {x} {y}");
        let (x, y, _) = xy(Rgb { r: 255, g: 255, b: 255 }).unwrap();
        assert!(near(x, 0.323) && near(y, 0.329), "Weiß: {x} {y}");
        let (x, y, bri) = xy(Rgb { r: 128, g: 0, b: 0 }).unwrap();
        assert!(near(x, 0.701) && near(y, 0.299), "halbes Rot behält den Farbton");
        assert!((bri - 50.0).abs() < 0.5, "halbes Rot: {bri}");
        assert_eq!(xy(Rgb::OFF), None);
    }

    #[test]
    fn debug_hides_the_key() {
        let s = format!("{:?}", hue(Kind::Light, "l1"));
        assert!(!s.contains("geheim") && s.contains("10.0.0.2"));
    }

    #[test]
    fn pairing_reply() {
        assert_eq!(parse_pair(&json!([{"success": {"username": "abc"}}])).unwrap(), "abc");
        let e = parse_pair(&json!([{"error": {"type": 101, "description": "link button not pressed"}}])).unwrap_err();
        assert!(e.starts_with("Knopf an der Bridge drücken"));
        assert_eq!(parse_pair(&json!([{"error": {"type": 7, "description": "x"}}])).unwrap_err(), "Hue-Bridge meldet: x");
        assert!(parse_pair(&json!({})).is_err());
    }

    #[test]
    fn targets_rooms_first_sorted_by_name() {
        let rooms = json!({"data": [
            {"id": "r2", "metadata": {"name": "flur"}},
            {"id": "r1", "metadata": {"name": "Büro"}},
        ]});
        let lights = json!({"data": [
            {"id": "l2", "metadata": {"name": "Stehlampe"}},
            {"id": "l1", "metadata": {"name": "Flur – Tür"}},
        ]});
        let t = parse_targets(&rooms, &lights);
        let names: Vec<(&str, Kind)> = t.iter().map(|(t, n)| (n.as_str(), t.kind)).collect();
        assert_eq!(names, [("Büro", Kind::Room), ("flur", Kind::Room), ("Flur – Tür", Kind::Light), ("Stehlampe", Kind::Light)]);
    }

    fn hue(kind: Kind, id: &str) -> Hue {
        Hue { host: "10.0.0.2".into(), key: "geheim".into(), fingerprint: "fp".into(), target: Target { kind, id: id.into() } }
    }

    fn light(id: &str, device: &str, on: bool, bri: f64, color: Value) -> Value {
        let mut l = json!({"id": id, "owner": {"rid": device, "rtype": "device"}, "on": {"on": on}, "dimming": {"brightness": bri}});
        for (k, v) in color.as_object().unwrap() {
            l[k] = v.clone();
        }
        l
    }

    /// Bridge mit einem Raum „r1“ (Geräte d1, d2, grouped_light g1) und drei Lampen
    struct Fake {
        log: Vec<(String, String, String, Option<Value>)>,
        lights: Vec<Value>,
        fail: bool,
    }

    impl Fake {
        fn new() -> Fake {
            Fake {
                log: vec![],
                lights: vec![
                    light("l1", "d1", true, 50.0, json!({"color_temperature": {"mirek": 366, "mirek_valid": true}, "color": {"xy": {"x": 0.45, "y": 0.41}}})),
                    light("l2", "d2", false, 80.0, json!({"color": {"xy": {"x": 0.2, "y": 0.3}}})),
                    light("l3", "d3", true, 20.0, json!({"color_temperature": {"mirek": null, "mirek_valid": false}, "color": {"xy": {"x": 0.6, "y": 0.3}}})),
                ],
                fail: false,
            }
        }

        fn send(&mut self) -> impl FnMut(&Hue, &str, &str, Option<Value>) -> Result<Value, String> + '_ {
            move |h, method, path, body| {
                if self.fail {
                    return Err("Hue-Bridge nicht erreichbar: aus".into());
                }
                self.log.push((h.target.id.clone(), method.into(), path.into(), body));
                let rest = path.strip_prefix("/clip/v2/resource/").unwrap();
                Ok(match (method, rest) {
                    ("GET", "light") => json!({"errors": [], "data": self.lights}),
                    ("GET", "room/r1") => json!({"data": [{
                        "id": "r1",
                        "children": [{"rid": "d1", "rtype": "device"}, {"rid": "d2", "rtype": "device"}],
                        "services": [{"rid": "g1", "rtype": "grouped_light"}],
                    }]}),
                    ("GET", r) if r.starts_with("light/") => {
                        let id = &r[6..];
                        json!({"data": self.lights.iter().filter(|l| l["id"] == id).collect::<Vec<_>>()})
                    }
                    ("GET", _) => json!({"data": []}),
                    _ => json!({"data": [], "errors": []}),
                })
            }
        }

        /// Befehle als „METHODE Pfad“ ohne Präfix
        fn take(&mut self) -> Vec<String> {
            self.log.drain(..).map(|(_, m, p, _)| format!("{m} {}", p.trim_start_matches("/clip/v2/resource/"))).collect()
        }

        fn bodies(&self) -> Vec<Value> {
            self.log.iter().filter_map(|(_, _, _, b)| b.clone()).collect()
        }
    }

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED, until: None };
    const CAM: Scene = Scene::Live { camera: true, color: Rgb::MAGENTA, until: None };

    #[test]
    fn single_light_colored_and_restored() {
        let mut f = Fake::new();
        let mut s = Session::default();
        let h = hue(Kind::Light, "l1");
        s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
        let body = f.bodies()[0].clone();
        assert_eq!(f.take(), ["GET light/l1", "PUT light/l1"]);
        assert_eq!(body["on"]["on"], true);
        assert!(near(body["color"]["xy"]["x"].as_f64().unwrap(), 0.701));
        assert!(near(body["dimming"]["brightness"].as_f64().unwrap(), 100.0));

        s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "gleiche Szene nicht erneut senden");
        s.apply_with(Some(&h), &CAM, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["PUT light/l1"], "Schnappschuss nur einmal");

        s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.bodies(), [json!({"on": {"on": true}, "dimming": {"brightness": 50.0}, "color_temperature": {"mirek": 366}})]);
        assert_eq!(f.take(), ["PUT light/l1"]);
        s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn switched_off_light_goes_back_off_and_xy_is_kept() {
        let mut f = Fake::new();
        let mut s = Session::default();
        for (id, want) in [
            (
                "l2",
                vec![
                    json!({"on": {"on": true}, "dimming": {"brightness": 80.0}, "color": {"xy": {"x": 0.2, "y": 0.3}}, "dynamics": {"duration": 0}}),
                    json!({"on": {"on": false}}),
                ],
            ),
            ("l3", vec![json!({"on": {"on": true}, "dimming": {"brightness": 20.0}, "color": {"xy": {"x": 0.6, "y": 0.3}}})]),
        ] {
            let h = hue(Kind::Light, id);
            s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
            f.log.clear();
            s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).unwrap();
            assert_eq!(f.bodies(), want, "{id}: erst Farbe und Helligkeit, dann aus");
            f.log.clear();
        }
    }

    #[test]
    fn room_colors_via_group_and_restores_each_light() {
        let mut f = Fake::new();
        let mut s = Session::default();
        let h = hue(Kind::Room, "r1");
        s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["GET room/r1", "GET light", "PUT grouped_light/g1"]);
        s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).unwrap();
        let bodies = f.bodies();
        assert_eq!(f.take(), ["PUT light/l1", "PUT light/l2", "PUT light/l2"], "nur Lampen der Geräte im Raum");
        assert_eq!(bodies[2], json!({"on": {"on": false}}));
    }

    #[test]
    fn room_without_lights_is_an_error_and_colors_nothing() {
        let mut f = Fake::new();
        let mut s = Session::default();
        let err = s.apply_with(Some(&hue(Kind::Room, "leer")), &LIVE, &mut f.send()).unwrap_err();
        assert!(err.contains("nicht gefunden"), "{err}");
        assert!(f.take().iter().all(|c| c.starts_with("GET")));
        s.apply_with(Some(&hue(Kind::Room, "leer")), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "nichts zurückzustellen");
    }

    #[test]
    fn failed_snapshot_colors_nothing_and_retries() {
        let mut f = Fake::new();
        f.fail = true;
        let mut s = Session::default();
        let h = hue(Kind::Light, "l1");
        assert!(s.apply_with(Some(&h), &LIVE, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
        s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["GET light/l1", "PUT light/l1"]);
    }

    #[test]
    fn failed_restore_is_repeated() {
        let mut f = Fake::new();
        let mut s = Session::default();
        let h = hue(Kind::Light, "l1");
        s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
        f.log.clear();
        f.fail = true;
        assert!(s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["PUT light/l1"]);
    }

    #[test]
    fn changing_or_clearing_the_target_restores_the_old_one() {
        let mut f = Fake::new();
        let mut s = Session::default();
        s.apply_with(Some(&hue(Kind::Light, "l1")), &LIVE, &mut f.send()).unwrap();
        f.log.clear();
        s.apply_with(Some(&hue(Kind::Light, "l3")), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["PUT light/l1", "GET light/l3", "PUT light/l3"]);
        assert_eq!(s.current(), Some(&hue(Kind::Light, "l3")));
        s.apply_with(None, &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), ["PUT light/l3"]);
        assert_eq!(s.current(), None);
    }

    #[test]
    fn off_light_without_known_state_only_goes_off() {
        let l = Saved { id: "x".into(), on: false, brightness: None, color: SavedColor::Unknown, supports_color: false, supports_dimming: false };
        assert_eq!(l.bodies(), [json!({"on": {"on": false}})]);
    }

    #[test]
    fn white_light_gets_no_color() {
        let mut f = Fake::new();
        f.lights.push(json!({"id": "w1", "owner": {"rid": "d9"}, "on": {"on": true}, "dimming": {"brightness": 60.0},
            "color_temperature": {"mirek": 250, "mirek_valid": true}}));
        let mut s = Session::default();
        let h = hue(Kind::Light, "w1");
        s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
        let b = f.bodies();
        assert_eq!(b[0], json!({"on": {"on": true}, "dimming": {"brightness": 100.0}}));
    }

    #[test]
    fn failed_restore_on_target_change_is_retried_and_not_lost() {
        let mut f = Fake::new();
        let mut s = Session::default();
        let room = hue(Kind::Room, "r1");
        s.apply_with(Some(&room), &LIVE, &mut f.send()).unwrap();
        f.log.clear();
        // Wechsel im Call auf eine Lampe des Raums, das Zurückstellen scheitert
        let mut calls = 0;
        let lamp = hue(Kind::Light, "l1");
        let r = s.apply_with(Some(&lamp), &LIVE, &mut |h, m, p, b| {
            calls += 1;
            // Zurückstellen (1) und der zweite Versuch nach dem neuen Ziel (4) scheitern
            if calls == 1 || calls == 4 {
                return Err("Hue-Bridge nicht erreichbar: timeout".into());
            }
            f.send()(h, m, p, b)
        });
        assert!(r.unwrap_err().starts_with("Frühere Hue-Lampe nicht zurückgestellt"), "Fehler im Menü, Updater wiederholt");
        let open: Vec<usize> = s.orphans.iter().map(|o| o.saved.len()).collect();
        assert_eq!(open, [1], "l1 gehört jetzt zum neuen Ziel, l2 bleibt offen");
        f.log.clear();
        // nach dem Call: l1 bekommt den Zustand von vor dem Call (nicht Rot),
        // l2 aus dem alten Raum wird beim nächsten Durchgang nachgeholt
        s.apply_with(Some(&lamp), &Scene::Idle, &mut f.send()).unwrap();
        let log: Vec<(String, Value)> = f.log.drain(..).map(|(_, _, p, b)| (p, b.unwrap())).collect();
        let l1: Vec<&Value> = log.iter().filter(|(p, _)| p.ends_with("/l1")).map(|(_, b)| b).collect();
        assert_eq!(l1, [&json!({"on": {"on": true}, "dimming": {"brightness": 50.0}, "color_temperature": {"mirek": 366}})]);
        assert!(log.iter().any(|(p, _)| p.ends_with("/l2")), "alte Lampe l2 nachgeholt");
        assert!(s.orphans.is_empty());
    }

    #[test]
    fn unreachable_old_target_is_given_up() {
        let mut f = Fake::new();
        let mut s = Session::default();
        s.apply_with(Some(&hue(Kind::Light, "l1")), &LIVE, &mut f.send()).unwrap();
        let mut other = hue(Kind::Light, "l1");
        other.host = "10.0.0.9".into();
        let mut send = |h: &Hue, m: &str, p: &str, b: Option<Value>| {
            if h.host == "10.0.0.2" {
                return Err("Hue-Bridge nicht erreichbar: timeout".to_string());
            }
            f.send()(h, m, p, b)
        };
        let mut errors = 0;
        for _ in 0..MAX_ORPHAN_TRIES + 2 {
            if s.apply_with(Some(&other), &LIVE, &mut send).is_err() {
                errors += 1;
            }
        }
        // fünf Versuche: beim Wechsel, sofort danach (Fehler), zwei weitere (Fehler), der letzte gibt auf
        assert_eq!(errors, MAX_ORPHAN_TRIES - 2);
        assert!(s.orphans.is_empty());
    }

    #[test]
    fn timer_minutes_do_not_resend() {
        let mut f = Fake::new();
        let mut s = Session::default();
        let h = hue(Kind::Light, "l1");
        s.apply_with(Some(&h), &Scene::Timer { minutes: 25, color: Rgb::RED }, &mut f.send()).unwrap();
        f.log.clear();
        s.apply_with(Some(&h), &Scene::Timer { minutes: 24, color: Rgb::RED }, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "gleiche Farbe, keine neuen Befehle");
    }

    #[test]
    fn plug_gets_neither_color_nor_dimming() {
        let mut f = Fake::new();
        f.lights.push(json!({"id": "p1", "owner": {"rid": "d8"}, "on": {"on": false}}));
        let mut s = Session::default();
        let h = hue(Kind::Light, "p1");
        s.apply_with(Some(&h), &LIVE, &mut f.send()).unwrap();
        s.apply_with(Some(&h), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.bodies(), [json!({"on": {"on": true}}), json!({"on": {"on": false}})]);
    }

    #[test]
    fn timer_and_focus_use_their_colors() {
        let mut f = Fake::new();
        let mut s = Session::default();
        let h = hue(Kind::Light, "l1");
        let violet = Rgb { r: 0x80, g: 0, b: 0xff };
        s.apply_with(Some(&h), &Scene::Timer { minutes: 5, color: violet }, &mut f.send()).unwrap();
        s.apply_with(Some(&h), &Scene::Focus { color: Rgb::GREEN }, &mut f.send()).unwrap();
        let b = f.bodies();
        let (vx, _, _) = xy(violet).unwrap();
        assert!(near(b[0]["color"]["xy"]["x"].as_f64().unwrap(), vx));
        assert!(near(b[1]["color"]["xy"]["x"].as_f64().unwrap(), 0.172));
    }
}
