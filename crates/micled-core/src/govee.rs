//! Govee-Lampen über die Steuerung im eigenen Netz („LAN Control“, in der
//! Govee-App je Gerät einzuschalten): Die Lampe zeigt die Farbe der [`Scene`]
//! und bekommt danach An/Aus, Helligkeit und Farbe von vorher zurück.
//!
//! JSON per UDP: Suche mit `scan` an `239.255.255.250:4001`, Befehle (`turn`,
//! `brightness`, `colorwc`, `devStatus`) an Port 4003 des Geräts. Antworten
//! kommen immer an Port 4002 des Absenders, deshalb nutzt MicLED den Port nur
//! für einen Vorgang und nie zwei Vorgänge gleichzeitig.
//!
//! Szenen und Bildschirm-Sync (DreamView) meldet das Gerät im eigenen Netz
//! nicht. War so etwas an, bekommt die Lampe danach eine feste Farbe.

use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::color::Rgb;
use crate::display::Scene;
use crate::output::{scene_color, Device, Keyed};

const SCAN_ADDR: &str = "239.255.255.250:4001";
const CONTROL_PORT: u16 = 4003;
const REPLY_PORT: u16 = 4002;
/// So lange wird auf die Antwort auf `devStatus` gewartet
const STATUS_WAIT: Duration = Duration::from_secs(1);
/// So lange sammelt die Suche Antworten
const SCAN_WAIT: Duration = Duration::from_secs(2);
/// Abstand zwischen zwei Befehlen an dasselbe Gerät
const SPACING: Duration = Duration::from_millis(50);

/// Weißton nach dem Call, wenn das Gerät weder Farbe noch Weißton meldet
/// (lief eine Szene oder DreamView), damit die Call-Farbe nicht bleibt
const FALLBACK_KELVIN: u64 = 4000;

pub const PORT_BUSY: &str = "Port 4002 belegt, läuft hier eine andere Govee-Steuerung?";

/// Port 4002 gehört immer nur einem Vorgang
static REPLY_LOCK: Mutex<()> = Mutex::new(());

/// Govee-Gerät im eigenen Netz
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Govee {
    /// IP-Adresse oder Name
    pub host: String,
}

impl Keyed for Govee {
    fn address(&self) -> &str {
        &self.host
    }
}

/// Adresse aus den Einstellungen bereinigen wie [`crate::pixoo::normalize`],
/// ein Port fällt weg (Govee nutzt feste Ports). Leer = aus.
pub fn normalize(host: &str) -> Option<String> {
    let h = crate::pixoo::normalize(host)?;
    let h = match h.rsplit_once(':') {
        Some((ip, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => ip.to_string(),
        _ => h,
    };
    (!h.is_empty()).then_some(h)
}

fn msg(cmd: &str, data: Value) -> Value {
    json!({ "msg": { "cmd": cmd, "data": data } })
}

fn turn(on: bool) -> Value {
    msg("turn", json!({ "value": u8::from(on) }))
}

fn brightness(percent: u8) -> Value {
    msg("brightness", json!({ "value": percent.clamp(1, 100) }))
}

fn colorwc(c: Rgb, kelvin: u64) -> Value {
    msg("colorwc", json!({ "color": { "r": c.r, "g": c.g, "b": c.b }, "colorTemInKelvin": kelvin }))
}

/// Befehle für die Farbe `c`: Farbe auf den größten Kanal hochgerechnet, die
/// Helligkeit aus dem größten Kanal. `OFF` schaltet aus.
fn color_commands(c: Rgb) -> Vec<Value> {
    let max = c.r.max(c.g).max(c.b);
    if max == 0 {
        return vec![turn(false)];
    }
    let scale = |v: u8| ((v as u32 * 255 + max as u32 / 2) / max as u32) as u8;
    let full = Rgb { r: scale(c.r), g: scale(c.g), b: scale(c.b) };
    let percent = ((max as u32 * 100 + 127) / 255) as u8;
    vec![turn(true), colorwc(full, 0), brightness(percent)]
}

/// `data` einer Antwort auf `devStatus`
fn status_data(v: &Value) -> Option<&Value> {
    (v["msg"]["cmd"] == "devStatus").then(|| &v["msg"]["data"])
}

/// Adresse und Modell aus einer Antwort auf `scan`
fn parse_scan(v: &Value) -> Option<(String, String)> {
    if v["msg"]["cmd"] != "scan" {
        return None;
    }
    let d = &v["msg"]["data"];
    let ip = d["ip"].as_str()?.to_string();
    Some((ip, d["sku"].as_str().unwrap_or("Govee").to_string()))
}

/// Zustand von vor dem Call
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Saved {
    on: bool,
    brightness: Option<u8>,
    color: Option<Rgb>,
    /// Weißton in Kelvin, 0 = Farbe
    kelvin: u64,
}

impl Saved {
    fn from_status(d: &Value) -> Saved {
        let c = &d["color"];
        let color = match (c["r"].as_u64(), c["g"].as_u64(), c["b"].as_u64()) {
            (Some(r), Some(g), Some(b)) if r + g + b > 0 => Some(Rgb { r: r.min(255) as u8, g: g.min(255) as u8, b: b.min(255) as u8 }),
            _ => None,
        };
        Saved {
            on: d["onOff"].as_u64() == Some(1),
            brightness: d["brightness"].as_u64().map(|b| b.clamp(1, 100) as u8),
            color,
            kelvin: d["colorTemInKelvin"].as_u64().unwrap_or(0),
        }
    }

    /// Befehle, die den gemerkten Zustand wiederherstellen. Eine vorher
    /// ausgeschaltete Lampe bekommt erst Farbe und Helligkeit und wird dann
    /// ausgeschaltet, damit sie beim nächsten Einschalten nicht in der
    /// Call-Farbe leuchtet.
    fn commands(&self) -> Vec<Value> {
        let mut out = Vec::new();
        if self.on {
            out.push(turn(true));
        }
        match (self.kelvin, self.color) {
            (k, _) if k > 0 => out.push(colorwc(Rgb::OFF, k)),
            (_, Some(c)) => out.push(colorwc(c, 0)),
            _ => out.push(colorwc(Rgb::OFF, FALLBACK_KELVIN)),
        }
        if let Some(b) = self.brightness {
            out.push(brightness(b));
        }
        if !self.on {
            out.push(turn(false));
        }
        out
    }
}

/// Schickt Befehle an das Gerät und fragt danach `devStatus` ab. Liefert
/// dessen `data`; ohne Antwort ist es ein Fehler.
type SendFn<'a> = dyn FnMut(&str, &[Value]) -> Result<Value, String> + 'a;

fn resolve(host: &str) -> Result<IpAddr, String> {
    (host, CONTROL_PORT)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.find(SocketAddr::is_ipv4))
        .map(|a| a.ip())
        .ok_or_else(|| format!("Govee {host}: Adresse unbekannt"))
}

fn bind() -> Result<UdpSocket, String> {
    UdpSocket::bind(("0.0.0.0", REPLY_PORT)).map_err(|e| match e.kind() {
        ErrorKind::AddrInUse => PORT_BUSY.to_string(),
        _ => format!("Govee: Port {REPLY_PORT} nicht nutzbar: {e}"),
    })
}

/// Antworten bis `deadline` lesen, `pick` entscheidet, was zählt
fn receive(sock: &UdpSocket, deadline: Instant, mut pick: impl FnMut(SocketAddr, Value) -> bool) {
    let mut buf = [0u8; 2048];
    while let Some(left) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
        if sock.set_read_timeout(Some(left)).is_err() {
            return;
        }
        let Ok((n, from)) = sock.recv_from(&mut buf) else { return };
        if let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) {
            if pick(from, v) {
                return;
            }
        }
    }
}

fn exchange(host: &str, commands: &[Value]) -> Result<Value, String> {
    let ip = resolve(host)?;
    let _port = REPLY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let sock = bind()?;
    let to = SocketAddr::new(ip, CONTROL_PORT);
    let send = |v: &Value| sock.send_to(v.to_string().as_bytes(), to).map_err(|e| format!("Govee {host} nicht erreichbar: {e}"));
    for c in commands {
        send(c)?;
        std::thread::sleep(SPACING);
    }
    send(&msg("devStatus", json!({})))?;
    let mut status = None;
    receive(&sock, Instant::now() + STATUS_WAIT, |from, v| {
        if from.ip() != ip {
            return false;
        }
        status = status_data(&v).cloned();
        status.is_some()
    });
    status.ok_or_else(|| format!("Govee {host} antwortet nicht"))
}

/// Geräte im eigenen Netz suchen (blockiert etwa zwei Sekunden). Liefert
/// Adresse und Modell, nach Adresse sortiert; keine Antwort ist kein Fehler.
pub fn discover() -> Result<Vec<(String, String)>, String> {
    let _port = REPLY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let sock = bind()?;
    let scan = msg("scan", json!({ "account_topic": "reserve" }));
    sock.send_to(scan.to_string().as_bytes(), SCAN_ADDR).map_err(|e| format!("Govee-Suche nicht möglich: {e}"))?;
    let mut found: Vec<(String, String)> = Vec::new();
    receive(&sock, Instant::now() + SCAN_WAIT, |_, v| {
        if let Some(d) = parse_scan(&v) {
            if !found.iter().any(|(ip, _)| *ip == d.0) {
                found.push(d);
            }
        }
        false
    });
    found.sort();
    Ok(found)
}

/// Zustand eines Geräts: was vorher war und ob gerade eine Farbe von MicLED
/// zu sehen ist
#[derive(Debug, Default)]
pub struct Session {
    cfg: Option<Govee>,
    /// `None` = noch kein Schnappschuss
    saved: Option<Saved>,
    /// Zuletzt sicher gezeigte Farbe
    shown: Option<Rgb>,
    /// Eine Farbe von MicLED ist womöglich zu sehen (auch nach einem Timeout)
    dirty: bool,
}

impl Device for Session {
    type Cfg = Govee;

    fn apply(&mut self, cfg: Option<&Govee>, scene: &Scene) -> Result<(), String> {
        self.apply_with(cfg, scene, &mut exchange)
    }

    fn restore(&mut self) -> Result<(), String> {
        let cfg = self.cfg.clone();
        self.apply(cfg.as_ref(), &Scene::Idle)
    }

    fn current(&self) -> Option<&Govee> {
        self.cfg.as_ref()
    }
}

impl Session {
    /// Wechselt das Ziel, wird das alte erst zurückgestellt. Scheitert das,
    /// bleibt der Zustand für einen weiteren Versuch erhalten.
    fn apply_with(&mut self, cfg: Option<&Govee>, scene: &Scene, send: &mut SendFn<'_>) -> Result<(), String> {
        if let Some(old) = self.cfg.clone().filter(|old| Some(old) != cfg) {
            self.idle(&old, send)?;
            *self = Session::default();
        }
        let Some(g) = cfg else { return Ok(()) };
        self.cfg = Some(g.clone());
        let Some(color) = scene_color(scene) else { return self.idle(g, send) };
        if self.shown == Some(color) {
            return Ok(());
        }
        if self.saved.is_none() {
            self.saved = Some(Saved::from_status(&send(&g.host, &[])?));
        }
        self.dirty = true;
        self.shown = None;
        send(&g.host, &color_commands(color))?;
        self.shown = Some(color);
        Ok(())
    }

    fn idle(&mut self, g: &Govee, send: &mut SendFn<'_>) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let commands = self.saved.map(|s| s.commands()).unwrap_or_default();
        send(&g.host, &commands)?;
        self.dirty = false;
        self.shown = None;
        self.saved = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn govee(host: &str) -> Govee {
        Govee { host: host.into() }
    }

    fn status(on: u8, bri: u8, rgb: (u8, u8, u8), kelvin: u64) -> Value {
        json!({"onOff": on, "brightness": bri, "color": {"r": rgb.0, "g": rgb.1, "b": rgb.2}, "colorTemInKelvin": kelvin})
    }

    /// Gerät, das Befehle mitschreibt und einen festen Zustand meldet
    struct Fake {
        log: Vec<(String, Vec<Value>)>,
        status: Value,
        fail: bool,
    }

    impl Fake {
        fn new(status: Value) -> Fake {
            Fake { log: vec![], status, fail: false }
        }

        fn send(&mut self) -> impl FnMut(&str, &[Value]) -> Result<Value, String> + '_ {
            move |host, cmds| {
                if self.fail {
                    return Err(format!("Govee {host} antwortet nicht"));
                }
                self.log.push((host.into(), cmds.to_vec()));
                Ok(self.status.clone())
            }
        }

        /// Befehle als „cmd“ je Austausch, ohne Adresse
        fn take(&mut self) -> Vec<Vec<String>> {
            self.log
                .drain(..)
                .map(|(_, c)| c.iter().map(|v| v["msg"]["cmd"].as_str().unwrap().to_string()).collect())
                .collect()
        }
    }

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED };

    #[test]
    fn address_without_port() {
        assert_eq!(normalize(" http://192.168.1.31:4003/ ").as_deref(), Some("192.168.1.31"));
        assert_eq!(normalize("lampe.local").as_deref(), Some("lampe.local"));
        assert_eq!(normalize(" "), None);
    }

    #[test]
    fn messages() {
        assert_eq!(turn(true), json!({"msg": {"cmd": "turn", "data": {"value": 1}}}));
        assert_eq!(brightness(0), json!({"msg": {"cmd": "brightness", "data": {"value": 1}}}));
        assert_eq!(
            colorwc(Rgb { r: 1, g: 2, b: 3 }, 0),
            json!({"msg": {"cmd": "colorwc", "data": {"color": {"r": 1, "g": 2, "b": 3}, "colorTemInKelvin": 0}}})
        );
    }

    #[test]
    fn color_is_scaled_to_full_and_brightness_follows_the_largest_channel() {
        let c = color_commands(Rgb { r: 128, g: 64, b: 0 });
        assert_eq!(c[0], turn(true));
        assert_eq!(c[1], colorwc(Rgb { r: 255, g: 128, b: 0 }, 0));
        assert_eq!(c[2], brightness(50));
        assert_eq!(color_commands(Rgb::RED)[2], brightness(100));
        assert_eq!(color_commands(Rgb::OFF), [turn(false)]);
    }

    #[test]
    fn reads_status_and_scan_replies() {
        let v = json!({"msg": {"cmd": "devStatus", "data": status(1, 40, (0, 0, 255), 0)}});
        let s = Saved::from_status(status_data(&v).unwrap());
        assert_eq!(s, Saved { on: true, brightness: Some(40), color: Some(Rgb { r: 0, g: 0, b: 255 }), kelvin: 0 });
        assert_eq!(status_data(&json!({"msg": {"cmd": "scan", "data": {}}})), None);
        let s = Saved::from_status(&json!({"onOff": 0}));
        assert_eq!(s, Saved { on: false, brightness: None, color: None, kelvin: 0 });

        let scan = json!({"msg": {"cmd": "scan", "data": {"ip": "192.168.1.31", "device": "1F:80", "sku": "H6609"}}});
        assert_eq!(parse_scan(&scan), Some(("192.168.1.31".into(), "H6609".into())));
        assert_eq!(parse_scan(&json!({"msg": {"cmd": "scan", "data": {}}})), None);
    }

    #[test]
    fn restore_commands() {
        let on_rgb = Saved { on: true, brightness: Some(40), color: Some(Rgb::GREEN), kelvin: 0 };
        assert_eq!(on_rgb.commands(), [turn(true), colorwc(Rgb::GREEN, 0), brightness(40)]);
        let on_white = Saved { on: true, brightness: Some(70), color: Some(Rgb::GREEN), kelvin: 2700 };
        assert_eq!(on_white.commands(), [turn(true), colorwc(Rgb::OFF, 2700), brightness(70)], "Weißton vor Farbe");
        let off = Saved { on: false, brightness: Some(20), color: Some(Rgb::GREEN), kelvin: 0 };
        assert_eq!(off.commands(), [colorwc(Rgb::GREEN, 0), brightness(20), turn(false)], "erst Farbe, dann aus");
        let unknown = Saved { on: false, brightness: None, color: None, kelvin: 0 };
        assert_eq!(unknown.commands(), [colorwc(Rgb::OFF, FALLBACK_KELVIN), turn(false)], "Szene oder DreamView: feste Farbe statt Call-Farbe");
    }

    #[test]
    fn colored_and_restored() {
        let mut f = Fake::new(status(1, 40, (0, 255, 0), 0));
        let mut s = Session::default();
        let g = govee("10.0.0.5");
        s.apply_with(Some(&g), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), [vec![], vec!["turn".to_string(), "colorwc".into(), "brightness".into()]], "erst Zustand, dann Farbe");
        s.apply_with(Some(&g), &Scene::Timer { minutes: 3, color: Rgb::RED }, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "gleiche Farbe nicht neu senden");
        s.apply_with(Some(&g), &Scene::Focus { color: Rgb::GREEN }, &mut f.send()).unwrap();
        assert_eq!(f.take().len(), 1, "Schnappschuss nur einmal");
        s.apply_with(Some(&g), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.log[0].1, [turn(true), colorwc(Rgb::GREEN, 0), brightness(40)]);
        f.take();
        s.apply_with(Some(&g), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn unreachable_device_is_an_error_and_retried() {
        let mut f = Fake::new(status(0, 80, (255, 255, 255), 4000));
        f.fail = true;
        let mut s = Session::default();
        let g = govee("10.0.0.5");
        assert_eq!(s.apply_with(Some(&g), &LIVE, &mut f.send()).unwrap_err(), "Govee 10.0.0.5 antwortet nicht");
        f.fail = false;
        s.apply_with(Some(&g), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "nichts gezeigt, nichts zurückzustellen");
        s.apply_with(Some(&g), &LIVE, &mut f.send()).unwrap();
        f.take();
        f.fail = true;
        assert!(s.apply_with(Some(&g), &Scene::Idle, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some(&g), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.log[0].1, [colorwc(Rgb::OFF, 4000), brightness(80), turn(false)], "Zurückstellen wiederholt");
    }

    #[test]
    fn removing_the_target_restores_it_and_keeps_state_on_failure() {
        let mut f = Fake::new(status(1, 100, (255, 0, 0), 0));
        let mut s = Session::default();
        let g = govee("10.0.0.5");
        s.apply_with(Some(&g), &LIVE, &mut f.send()).unwrap();
        f.take();
        f.fail = true;
        assert!(s.apply_with(None, &Scene::Idle, &mut f.send()).is_err());
        assert_eq!(s.current(), Some(&g), "Zustand bleibt für den nächsten Versuch");
        f.fail = false;
        s.apply_with(None, &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take().len(), 1);
        assert_eq!(s.current(), None);
    }
}
