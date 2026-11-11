//! „Auf Sendung“ per MQTT an Home Assistant, mit MQTT-Discovery: MicLED
//! erscheint dort als Gerät „MicLED (<Mac-Name>)“ mit den Sensoren Auf
//! Sendung, Kamera und Programm.
//!
//! Ohne MQTT-Bibliothek: MicLED braucht nur CONNECT, PUBLISH mit QoS 0 und
//! retain, PINGREQ und DISCONNECT aus MQTT 3.1.1. Die Pakete entstehen in
//! reinen Funktionen, die Verbindung hält [`Updater`] in einem eigenen Thread,
//! damit ein fehlender Broker die Lampe nicht aufhält.
//!
//! Verfügbarkeit: Beim Verbinden hinterlegt MicLED als „Last Will“ `offline`.
//! Endet MicLED, schläft der Mac oder bricht die Verbindung ab, zeigt Home
//! Assistant das Gerät als nicht verfügbar. Ausgeschaltet entfernt MicLED das
//! Gerät wieder (leere retained Nachrichten auf die Discovery-Topics).

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde_json::{json, Value};

/// Standard-Port ohne bzw. mit TLS
pub const PORT: u16 = 1883;
pub const TLS_PORT: u16 = 8883;
/// Keepalive im CONNECT; der Broker meldet MicLED nach dem Anderthalbfachen ab
const KEEPALIVE_S: u16 = 30;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Wartezeit beim Lesen und Schreiben (CONNACK, PINGRESP)
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Größtes Paket, das MicLED vom Broker annimmt
const MAX_PACKET: usize = 64 * 1024;
/// Home Assistant nimmt höchstens 255 Zeichen als Zustand eines Sensors
const MAX_STATE: usize = 255;

/// Zugang zum Broker und Name des Geräts in Home Assistant
#[derive(Clone, PartialEq, Eq)]
pub struct Mqtt {
    pub host: String,
    pub port: u16,
    /// Zertifikat gegen die öffentlichen Zertifizierungsstellen prüfen
    pub tls: bool,
    /// Leer = ohne Anmeldung
    pub user: String,
    pub password: String,
    /// Feste ID dieses Macs (nur Buchstaben, Ziffern, `-` und `_`)
    pub device_id: String,
    /// Name des Macs, erscheint als „MicLED (<Name>)“
    pub device_name: String,
}

impl std::fmt::Debug for Mqtt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mqtt")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("user", &self.user)
            .field("device_id", &self.device_id)
            .field("device_name", &self.device_name)
            .finish_non_exhaustive()
    }
}

/// Was MicLED an Home Assistant meldet
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub on_air: bool,
    pub camera: bool,
    /// Prozessnamen der Programme am Mikrofon
    pub apps: Vec<String>,
}

/// Adresse ohne `mqtt://`, `mqtts://`, eckige Klammern und `/` am Ende;
/// `None`, wenn nichts übrig bleibt
pub fn normalize(host: &str) -> Option<String> {
    let mut h = host.trim();
    for prefix in ["mqtt://", "mqtts://", "tcp://", "ssl://"] {
        if let Some(rest) = h.strip_prefix(prefix) {
            h = rest;
        }
    }
    let h = h.trim_end_matches('/').trim();
    let h = h.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or(h);
    (!h.is_empty()).then(|| h.to_string())
}

/// Geräte-ID für Topics: nur Buchstaben, Ziffern, `-` und `_`; `None`, wenn nichts übrig bleibt
pub fn clean_id(id: &str) -> Option<String> {
    let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (!id.is_empty()).then_some(id)
}

// --- Topics und Nachrichten ----------------------------------------------

pub fn state_topic(cfg: &Mqtt) -> String {
    format!("micled/{}/state", cfg.device_id)
}

pub fn availability_topic(cfg: &Mqtt) -> String {
    format!("micled/{}/availability", cfg.device_id)
}

fn object_id(cfg: &Mqtt) -> String {
    format!("micled_{}", cfg.device_id)
}

/// Programme für den Sensor „Programm“
pub fn program(apps: &[String]) -> String {
    if apps.is_empty() {
        return "keins".into();
    }
    let joined = apps.join(", ");
    match joined.char_indices().nth(MAX_STATE) {
        Some((cut, _)) => joined[..cut].to_string(),
        None => joined,
    }
}

/// Zustand als JSON für das State-Topic
pub fn state_payload(r: &Report) -> Vec<u8> {
    json!({ "on_air": r.on_air, "camera": r.camera, "program": program(&r.apps), "apps": r.apps })
        .to_string()
        .into_bytes()
}

/// Topics und Nachrichten der drei Discovery-Einträge (Auf Sendung, Kamera, Programm)
pub fn discovery(cfg: &Mqtt) -> Vec<(String, Value)> {
    let node = object_id(cfg);
    let device = json!({
        "identifiers": [node],
        "name": format!("MicLED ({})", cfg.device_name),
        "model": "MicLED",
        "sw_version": env!("CARGO_PKG_VERSION"),
    });
    let entry = |object: &str, name: &str, template: &str, icon: &str| {
        json!({
            "name": name,
            "unique_id": format!("{node}_{object}"),
            "state_topic": state_topic(cfg),
            "value_template": template,
            "availability_topic": availability_topic(cfg),
            "payload_available": "online",
            "payload_not_available": "offline",
            "icon": icon,
            "device": device,
        })
    };
    let mut on_air = entry("on_air", "Auf Sendung", "{{ 'ON' if value_json.on_air else 'OFF' }}", "mdi:broadcast");
    let mut camera = entry("camera", "Kamera", "{{ 'ON' if value_json.camera else 'OFF' }}", "mdi:webcam");
    for v in [&mut on_air, &mut camera] {
        v["payload_on"] = "ON".into();
        v["payload_off"] = "OFF".into();
    }
    let program = entry("program", "Programm", "{{ value_json.program }}", "mdi:application");
    vec![
        (format!("homeassistant/binary_sensor/{node}/on_air/config"), on_air),
        (format!("homeassistant/binary_sensor/{node}/camera/config"), camera),
        (format!("homeassistant/sensor/{node}/program/config"), program),
    ]
}

// --- Pakete (MQTT 3.1.1) --------------------------------------------------

/// Restlänge als Varint (höchstens vier Bytes)
pub fn encode_length(mut n: usize, out: &mut Vec<u8>) {
    loop {
        let mut b = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
}

fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u16).to_be_bytes());
    out.extend_from_slice(b);
}

fn packet(header: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![header];
    encode_length(body.len(), &mut out);
    out.extend_from_slice(body);
    out
}

/// Last Will: Topic und Nachricht, beide mit QoS 0 und retain
pub struct Will<'a> {
    pub topic: &'a str,
    pub message: &'a [u8],
}

/// CONNECT mit Clean Session. Benutzer und Passwort nur, wenn nicht leer
/// (ein Passwort ohne Benutzer erlaubt MQTT 3.1.1 nicht).
pub fn connect_packet(client_id: &str, will: Option<&Will>, user: &str, password: &str, keepalive_s: u16) -> Vec<u8> {
    let with_user = !user.is_empty();
    let with_password = with_user && !password.is_empty();
    let mut flags = 0x02; // Clean Session
    if will.is_some() {
        flags |= 0x04 | 0x20; // Will, QoS 0, Will Retain
    }
    if with_password {
        flags |= 0x40;
    }
    if with_user {
        flags |= 0x80;
    }
    let mut body = Vec::new();
    put_bytes(&mut body, b"MQTT");
    body.push(4); // Protokoll 3.1.1
    body.push(flags);
    body.extend_from_slice(&keepalive_s.to_be_bytes());
    put_bytes(&mut body, client_id.as_bytes());
    if let Some(w) = will {
        put_bytes(&mut body, w.topic.as_bytes());
        put_bytes(&mut body, w.message);
    }
    if with_user {
        put_bytes(&mut body, user.as_bytes());
    }
    if with_password {
        put_bytes(&mut body, password.as_bytes());
    }
    packet(0x10, &body)
}

/// PUBLISH mit QoS 0 und retain (leere Nachricht löscht die gespeicherte)
pub fn publish_packet(topic: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    put_bytes(&mut body, topic.as_bytes());
    body.extend_from_slice(payload);
    packet(0x31, &body)
}

pub const PINGREQ: [u8; 2] = [0xC0, 0x00];
pub const DISCONNECT: [u8; 2] = [0xE0, 0x00];
const CONNACK: u8 = 0x20;
const PINGRESP: u8 = 0xD0;

/// Ein Paket lesen: erstes Byte und Inhalt
pub fn read_packet(r: &mut impl Read) -> std::io::Result<(u8, Vec<u8>)> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    let header = b[0];
    let mut len = 0usize;
    let mut shift = 0;
    loop {
        r.read_exact(&mut b)?;
        len |= ((b[0] & 0x7f) as usize) << shift;
        if b[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "Länge ungültig"));
        }
    }
    if len > MAX_PACKET {
        return Err(std::io::Error::new(ErrorKind::InvalidData, "Paket zu groß"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok((header, body))
}

/// Antwort auf CONNECT auswerten
pub fn connack(header: u8, body: &[u8]) -> Result<(), String> {
    if header & 0xF0 != CONNACK || body.len() != 2 {
        return Err("MQTT: unerwartete Antwort des Brokers".into());
    }
    match body[1] {
        0 => Ok(()),
        4 => Err("MQTT: Benutzer oder Passwort falsch".into()),
        5 => Err("MQTT: nicht berechtigt".into()),
        c => Err(format!("MQTT: Broker lehnt ab ({c})")),
    }
}

// --- Verbindung -----------------------------------------------------------

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

/// Verbindung zum Broker nach erfolgreichem CONNACK
trait Link: Send {
    fn write(&mut self, packet: &[u8]) -> Result<(), String>;
    /// PINGREQ senden und auf PINGRESP warten
    fn ping(&mut self) -> Result<(), String>;
}

struct Net {
    stream: Box<dyn Stream>,
    tls: bool,
}

fn unreachable(e: &dyn std::fmt::Display) -> String {
    format!("MQTT: Broker nicht erreichbar ({e})")
}

impl Net {
    fn io_error(&self, e: std::io::Error) -> String {
        if self.tls && e.kind() == ErrorKind::InvalidData {
            format!("MQTT: TLS-Verbindung abgelehnt ({e})")
        } else {
            unreachable(&e)
        }
    }

    /// TCP (und TLS) aufbauen, CONNECT senden und CONNACK auswerten
    fn open(cfg: &Mqtt, client_id: &str, will: Option<&Will>) -> Result<Net, String> {
        let mut last: Option<std::io::Error> = None;
        let mut tcp = None;
        for addr in (cfg.host.as_str(), cfg.port).to_socket_addrs().map_err(|e| unreachable(&e))? {
            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                Ok(t) => {
                    tcp = Some(t);
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        let tcp = match (tcp, last) {
            (Some(t), _) => t,
            (None, Some(e)) => return Err(unreachable(&e)),
            (None, None) => return Err(unreachable(&"Adresse unbekannt")),
        };
        tcp.set_read_timeout(Some(IO_TIMEOUT)).map_err(|e| unreachable(&e))?;
        tcp.set_write_timeout(Some(IO_TIMEOUT)).map_err(|e| unreachable(&e))?;
        tcp.set_nodelay(true).map_err(|e| unreachable(&e))?;
        let stream: Box<dyn Stream> = if cfg.tls {
            let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
            let config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| unreachable(&e))?
                .with_root_certificates(roots)
                .with_no_client_auth();
            let name = ServerName::try_from(cfg.host.clone()).map_err(|_| format!("MQTT: ungültige Adresse {}", cfg.host))?;
            let conn = ClientConnection::new(Arc::new(config), name).map_err(|e| unreachable(&e))?;
            Box::new(StreamOwned::new(conn, tcp))
        } else {
            Box::new(tcp)
        };
        let mut net = Net { stream, tls: cfg.tls };
        net.write(&connect_packet(client_id, will, &cfg.user, &cfg.password, KEEPALIVE_S))?;
        let (header, body) = read_packet(&mut net.stream).map_err(|e| match e.kind() {
            ErrorKind::UnexpectedEof => "MQTT: Broker hat die Verbindung geschlossen".to_string(),
            _ => net.io_error(e),
        })?;
        connack(header, &body)?;
        Ok(net)
    }
}

impl Link for Net {
    fn write(&mut self, packet: &[u8]) -> Result<(), String> {
        self.stream.write_all(packet).and_then(|()| self.stream.flush()).map_err(|e| self.io_error(e))
    }

    fn ping(&mut self) -> Result<(), String> {
        self.write(&PINGREQ)?;
        loop {
            let (header, _) = read_packet(&mut self.stream).map_err(|e| self.io_error(e))?;
            if header & 0xF0 == PINGRESP {
                return Ok(());
            }
        }
    }
}

/// Zugang prüfen: verbinden, CONNACK auswerten, abmelden. Nutzt eine eigene
/// Client-ID, damit eine laufende Verbindung von MicLED bestehen bleibt.
pub fn check(cfg: &Mqtt) -> Result<(), String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let client_id = format!("micled-check-{}-{nanos}", std::process::id());
    let mut net = Net::open(cfg, &client_id, None)?;
    let _ = net.write(&DISCONNECT);
    Ok(())
}

// --- Ablauf ---------------------------------------------------------------

/// Zeiten des Ablaufs (in den Tests kürzer)
#[derive(Clone, Copy, Debug)]
struct Timing {
    /// Ruhe nach einer Änderung der Einstellungen (Tippen nicht senden)
    settle: Duration,
    /// Abstand der Versuche nach einem Fehler
    retry: Duration,
    /// So lange ohne Senden, dann PINGREQ
    idle: Duration,
}

const TIMING: Timing =
    Timing { settle: Duration::from_secs(2), retry: Duration::from_secs(10), idle: Duration::from_secs(15) };

type Connector = Box<dyn FnMut(&Mqtt) -> Result<Box<dyn Link>, String> + Send>;

fn net_connector() -> Connector {
    Box::new(|cfg: &Mqtt| {
        let topic = availability_topic(cfg);
        let will = Will { topic: &topic, message: b"offline" };
        Net::open(cfg, &format!("micled-{}", cfg.device_id), Some(&will)).map(|n| Box::new(n) as Box<dyn Link>)
    })
}

/// Verbindung und Stand beim Broker, ohne Thread und ohne Uhr
struct Session {
    connect: Connector,
    timing: Timing,
    /// Verbindung mit der Einstellung, mit der sie aufgebaut wurde
    link: Option<(Mqtt, Box<dyn Link>)>,
    /// Zuletzt gesendeter Zustand
    last_state: Option<Vec<u8>>,
    last_send: Instant,
    /// Nächster Versuch nach einem Fehler, nur für diese Einstellung
    next_try: Option<(Mqtt, Instant)>,
}

impl Session {
    fn new(connect: Connector, timing: Timing) -> Session {
        Session { connect, timing, link: None, last_state: None, last_send: Instant::now(), next_try: None }
    }

    /// Gewünschten Zustand herstellen. Liefert das Ergebnis, falls etwas
    /// versucht wurde (`None` = nichts zu tun oder Pause nach einem Fehler).
    fn step(&mut self, cfg: Option<&Mqtt>, report: &Report, asleep: bool, now: Instant) -> Option<Result<(), String>> {
        // (aus, Abschied nötig) für eine bestehende Verbindung
        let leave = self.link.as_ref().map(|(current, _)| (cfg.is_none(), asleep || cfg != Some(current)));
        match leave {
            Some((true, _)) => self.remove(),
            Some((false, true)) => self.goodbye(),
            _ => {}
        }
        let Some(cfg) = cfg.filter(|_| !asleep) else {
            self.next_try = None;
            return Some(Ok(()));
        };
        if self.link.is_none() {
            if matches!(&self.next_try, Some((c, t)) if c == cfg && now < *t) {
                return None;
            }
            let r = self.open(cfg, report, now);
            self.next_try = r.is_err().then(|| (cfg.clone(), now + self.timing.retry));
            return Some(r);
        }
        let state = state_payload(report);
        let r = if self.last_state.as_ref() != Some(&state) {
            self.publish(&state_topic(cfg), &state, now).map(|()| self.last_state = Some(state))
        } else if now.duration_since(self.last_send) >= self.timing.idle {
            self.ping(now)
        } else {
            return None;
        };
        if r.is_err() {
            // gleich neu verbinden, z. B. nach einem Neustart des Brokers
            self.link = None;
        }
        Some(r)
    }

    /// Wann `step` wieder dran ist (`None` = erst bei einer Nachricht)
    fn next_wake(&self, cfg: Option<&Mqtt>, asleep: bool, now: Instant) -> Option<Instant> {
        match (&self.link, cfg.filter(|_| !asleep)) {
            (Some(_), None) => Some(now),
            (None, None) => None,
            (Some((current, _)), Some(wanted)) if current != wanted => Some(now),
            (Some(_), Some(_)) => Some(self.last_send + self.timing.idle),
            (None, Some(wanted)) => match &self.next_try {
                Some((c, t)) if c == wanted => Some(*t),
                _ => Some(now),
            },
        }
    }

    fn open(&mut self, cfg: &Mqtt, report: &Report, now: Instant) -> Result<(), String> {
        let link = (self.connect)(cfg)?;
        self.link = Some((cfg.clone(), link));
        self.last_state = None;
        let r = (|| {
            for (topic, payload) in discovery(cfg) {
                self.publish(&topic, payload.to_string().as_bytes(), now)?;
            }
            self.publish(&availability_topic(cfg), b"online", now)?;
            let state = state_payload(report);
            self.publish(&state_topic(cfg), &state, now)?;
            self.last_state = Some(state);
            Ok(())
        })();
        if r.is_err() {
            self.link = None;
        }
        r
    }

    fn publish(&mut self, topic: &str, payload: &[u8], now: Instant) -> Result<(), String> {
        let (_, link) = self.link.as_mut().ok_or("MQTT: nicht verbunden")?;
        link.write(&publish_packet(topic, payload))?;
        self.last_send = now;
        Ok(())
    }

    fn ping(&mut self, now: Instant) -> Result<(), String> {
        let (_, link) = self.link.as_mut().ok_or("MQTT: nicht verbunden")?;
        link.ping()?;
        self.last_send = now;
        Ok(())
    }

    /// `offline` melden und abmelden (Ruhezustand, andere Einstellung, Ende)
    fn goodbye(&mut self) {
        if let Some((cfg, mut link)) = self.link.take() {
            let r = link.write(&publish_packet(&availability_topic(&cfg), b"offline")).and_then(|()| link.write(&DISCONNECT));
            if let Err(e) = r {
                log::warn!("{e}");
            }
        }
    }

    /// Gerät aus Home Assistant entfernen und abmelden (MQTT ausgeschaltet)
    fn remove(&mut self) {
        if let Some((cfg, mut link)) = self.link.take() {
            let mut topics: Vec<String> = discovery(&cfg).into_iter().map(|(t, _)| t).collect();
            topics.push(state_topic(&cfg));
            topics.push(availability_topic(&cfg));
            let r = topics
                .iter()
                .try_for_each(|t| link.write(&publish_packet(t, b"")))
                .and_then(|()| link.write(&DISCONNECT));
            if let Err(e) = r {
                log::warn!("{e}");
            }
        }
    }
}

enum Msg {
    Update(Option<Mqtt>, Report, bool),
    /// `offline` melden, abmelden und bestätigen (beim Beenden der App)
    Shutdown(Sender<()>),
}

/// Hält die Verbindung zum Broker in einem eigenen Thread. `send` kehrt
/// sofort zurück; Fehler landen im Fehler-Slot, ein Erfolg leert ihn.
pub struct Updater {
    tx: Sender<Msg>,
}

impl Updater {
    pub fn start(error: Arc<Mutex<Option<String>>>) -> Updater {
        Updater::start_with(error, net_connector(), TIMING)
    }

    fn start_with(error: Arc<Mutex<Option<String>>>, connect: Connector, timing: Timing) -> Updater {
        let (tx, rx) = channel::<Msg>();
        std::thread::Builder::new()
            .name("micled-mqtt".into())
            .spawn(move || {
                let mut session = Session::new(connect, timing);
                // zuletzt gewünschte und (nach der Ruhezeit) geltende Einstellung
                let mut pending: Option<Mqtt> = None;
                let mut active: Option<Mqtt> = None;
                let mut settle_at: Option<Instant> = None;
                let mut report = Report::default();
                let mut asleep = false;
                loop {
                    let now = Instant::now();
                    let wake = [settle_at, session.next_wake(active.as_ref(), asleep, now)].into_iter().flatten().min();
                    let wait = wake.map(|t| t.saturating_duration_since(now)).unwrap_or(Duration::from_secs(3600));
                    match rx.recv_timeout(wait) {
                        Ok(Msg::Update(cfg, r, a)) => {
                            if cfg != pending {
                                pending = cfg;
                                settle_at = Some(Instant::now() + timing.settle);
                            }
                            report = r;
                            asleep = a;
                        }
                        Ok(Msg::Shutdown(ack)) => {
                            session.goodbye();
                            let _ = ack.send(());
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            session.goodbye();
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    if settle_at.is_some_and(|t| Instant::now() >= t) {
                        active = pending.clone();
                        settle_at = None;
                    }
                    if let Some(r) = session.step(active.as_ref(), &report, asleep, Instant::now()) {
                        if let Err(e) = &r {
                            log::warn!("{e}");
                        }
                        *error.lock().unwrap() = r.err();
                    }
                }
            })
            .expect("MQTT-Thread");
        Updater { tx }
    }

    /// Gewünschter Zustand: Einstellung (`None` = aus), Meldung und Ruhezustand
    pub fn send(&self, cfg: Option<Mqtt>, report: Report, asleep: bool) {
        let _ = self.tx.send(Msg::Update(cfg, report, asleep));
    }

    /// Abmelden anstoßen, ohne zu warten. Die Rückgabe meldet das Ende
    /// (`None`, wenn der Thread schon beendet ist).
    pub fn begin_shutdown(&self) -> Option<Receiver<()>> {
        let (ack, done) = channel();
        self.tx.send(Msg::Shutdown(ack)).ok().map(|()| done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;

    fn cfg() -> Mqtt {
        Mqtt {
            host: "127.0.0.1".into(),
            port: PORT,
            tls: false,
            user: "micled".into(),
            password: "geheim".into(),
            device_id: "a1b2c3d4e5f6".into(),
            device_name: "Franks MacBook".into(),
        }
    }

    fn live(apps: &[&str]) -> Report {
        Report { on_air: true, camera: false, apps: apps.iter().map(|s| s.to_string()).collect() }
    }

    // --- Pakete ------------------------------------------------------------

    fn decode_length(b: &[u8]) -> usize {
        let mut r = Cursor::new([&[0x30], b, &vec![0u8; 400][..]].concat());
        read_packet(&mut r).unwrap().1.len()
    }

    #[test]
    fn remaining_length_as_varint() {
        for (n, bytes) in [(0, vec![0x00]), (127, vec![0x7f]), (128, vec![0x80, 0x01]), (321, vec![0xC1, 0x02])] {
            let mut out = Vec::new();
            encode_length(n, &mut out);
            assert_eq!(out, bytes, "{n}");
            assert_eq!(decode_length(&bytes), n);
        }
        let mut out = Vec::new();
        encode_length(268_435_455, &mut out);
        assert_eq!(out, [0xFF, 0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn connect_with_will_and_login() {
        let will = Will { topic: "micled/x/availability", message: b"offline" };
        let p = connect_packet("micled-x", Some(&will), "u", "pw", 30);
        let (header, body) = read_packet(&mut Cursor::new(&p)).unwrap();
        assert_eq!(header, 0x10);
        assert_eq!(&body[..7], b"\x00\x04MQTT\x04");
        assert_eq!(body[7], 0x80 | 0x40 | 0x20 | 0x04 | 0x02, "Benutzer, Passwort, Will retain, Will, Clean Session");
        assert_eq!(&body[8..10], &[0, 30], "Keepalive");
        let rest = &body[10..];
        let expected = [
            &b"\x00\x08micled-x"[..],
            b"\x00\x15micled/x/availability",
            b"\x00\x07offline",
            b"\x00\x01u",
            b"\x00\x02pw",
        ]
        .concat();
        assert_eq!(rest, &expected[..]);
    }

    #[test]
    fn connect_without_login_or_will() {
        let p = connect_packet("c", None, "", "pw", 30);
        let (_, body) = read_packet(&mut Cursor::new(&p)).unwrap();
        assert_eq!(body[7], 0x02, "ohne Benutzer auch kein Passwort");
        assert_eq!(&body[10..], b"\x00\x01c");
        let p = connect_packet("c", None, "u", "", 30);
        let (_, body) = read_packet(&mut Cursor::new(&p)).unwrap();
        assert_eq!(body[7], 0x80 | 0x02);
    }

    #[test]
    fn publish_is_retained_qos0() {
        let p = publish_packet("a/b", b"on");
        assert_eq!(p, [0x31, 7, 0, 3, b'a', b'/', b'b', b'o', b'n']);
        assert_eq!(publish_packet("t", b""), [0x31, 3, 0, 1, b't'], "leer löscht die gespeicherte Nachricht");
        assert_eq!(PINGREQ, [0xC0, 0]);
        assert_eq!(DISCONNECT, [0xE0, 0]);
    }

    #[test]
    fn connack_codes() {
        assert_eq!(connack(0x20, &[0, 0]), Ok(()));
        assert_eq!(connack(0x20, &[0, 4]).unwrap_err(), "MQTT: Benutzer oder Passwort falsch");
        assert_eq!(connack(0x20, &[0, 5]).unwrap_err(), "MQTT: nicht berechtigt");
        assert_eq!(connack(0x20, &[0, 3]).unwrap_err(), "MQTT: Broker lehnt ab (3)");
        assert!(connack(0xD0, &[]).is_err());
    }

    #[test]
    fn oversized_packet_is_rejected() {
        let mut bytes = vec![0x30];
        encode_length(MAX_PACKET + 1, &mut bytes);
        assert!(read_packet(&mut Cursor::new(bytes)).is_err());
    }

    // --- Nachrichten ---------------------------------------------------------

    #[test]
    fn state_json() {
        let v: Value = serde_json::from_slice(&state_payload(&live(&["zoom.us", "Plaud"]))).unwrap();
        assert_eq!(v, json!({"on_air": true, "camera": false, "program": "zoom.us, Plaud", "apps": ["zoom.us", "Plaud"]}));
        let v: Value = serde_json::from_slice(&state_payload(&Report::default())).unwrap();
        assert_eq!(v["program"], "keins");
        assert_eq!(v["on_air"], false);
    }

    #[test]
    fn program_is_cut_for_home_assistant() {
        let long = vec!["ä".repeat(300)];
        assert_eq!(program(&long).chars().count(), MAX_STATE);
    }

    #[test]
    fn discovery_payloads() {
        let list = discovery(&cfg());
        let topics: Vec<&str> = list.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            topics,
            [
                "homeassistant/binary_sensor/micled_a1b2c3d4e5f6/on_air/config",
                "homeassistant/binary_sensor/micled_a1b2c3d4e5f6/camera/config",
                "homeassistant/sensor/micled_a1b2c3d4e5f6/program/config",
            ]
        );
        let names: Vec<&str> = list.iter().map(|(_, v)| v["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Auf Sendung", "Kamera", "Programm"]);
        for (_, v) in &list {
            assert_eq!(v["state_topic"], "micled/a1b2c3d4e5f6/state");
            assert_eq!(v["availability_topic"], "micled/a1b2c3d4e5f6/availability");
            assert_eq!(v["device"]["identifiers"], json!(["micled_a1b2c3d4e5f6"]));
            assert_eq!(v["device"]["name"], "MicLED (Franks MacBook)");
            assert_eq!(v["device"]["model"], "MicLED");
            assert_eq!(v["device"]["sw_version"], env!("CARGO_PKG_VERSION"));
        }
        assert_eq!(list[0].1["unique_id"], "micled_a1b2c3d4e5f6_on_air");
        assert_eq!(list[0].1["value_template"], "{{ 'ON' if value_json.on_air else 'OFF' }}");
        assert_eq!(list[1].1["value_template"], "{{ 'ON' if value_json.camera else 'OFF' }}");
        assert_eq!(list[2].1["unique_id"], "micled_a1b2c3d4e5f6_program");
        assert_eq!(list[2].1["value_template"], "{{ value_json.program }}");
        assert!(list[2].1.get("payload_on").is_none());
    }

    #[test]
    fn password_not_in_debug_output() {
        assert!(!format!("{:?}", cfg()).contains("geheim"));
    }

    #[test]
    fn host_and_id_are_cleaned() {
        assert_eq!(normalize(" mqtt://homeassistant.local/ ").as_deref(), Some("homeassistant.local"));
        assert_eq!(normalize("[fd00::1]").as_deref(), Some("fd00::1"));
        assert_eq!(normalize("  "), None);
        assert_eq!(clean_id("a1/b2+#c3").as_deref(), Some("a1b2c3"));
        assert_eq!(clean_id("/#"), None);
    }

    // --- Ablauf mit nachgebildeter Verbindung -------------------------------

    /// Mitschrift aller Verbindungen: je Verbindung die gesendeten Pakete
    #[derive(Default)]
    struct Wire {
        connections: Vec<Vec<Vec<u8>>>,
        pings: usize,
        /// Nächster Verbindungsversuch scheitert
        refuse: bool,
        /// Senden scheitert
        broken: bool,
    }

    struct Fake(Arc<Mutex<Wire>>);

    impl Link for Fake {
        fn write(&mut self, packet: &[u8]) -> Result<(), String> {
            let mut w = self.0.lock().unwrap();
            if w.broken {
                return Err("MQTT: Broker nicht erreichbar (kaputt)".into());
            }
            w.connections.last_mut().unwrap().push(packet.to_vec());
            Ok(())
        }
        fn ping(&mut self) -> Result<(), String> {
            let mut w = self.0.lock().unwrap();
            w.pings += 1;
            if w.broken {
                return Err("MQTT: Broker nicht erreichbar (kaputt)".into());
            }
            Ok(())
        }
    }

    const FAST: Timing =
        Timing { settle: Duration::from_millis(50), retry: Duration::from_secs(10), idle: Duration::from_secs(15) };

    fn session() -> (Session, Arc<Mutex<Wire>>) {
        let wire = Arc::new(Mutex::new(Wire::default()));
        let w = wire.clone();
        let connect: Connector = Box::new(move |_cfg: &Mqtt| {
            let mut wire = w.lock().unwrap();
            if wire.refuse {
                return Err("MQTT: Broker nicht erreichbar (refused)".into());
            }
            wire.connections.push(Vec::new());
            Ok(Box::new(Fake(w.clone())) as Box<dyn Link>)
        });
        (Session::new(connect, FAST), wire)
    }

    /// Topic und Nachricht eines PUBLISH, `None` bei anderen Paketen
    fn topic_of(p: &[u8]) -> Option<(String, String)> {
        let (header, body) = read_packet(&mut Cursor::new(p)).ok()?;
        if header != 0x31 {
            return None;
        }
        let len = u16::from_be_bytes([body[0], body[1]]) as usize;
        Some((String::from_utf8_lossy(&body[2..2 + len]).into(), String::from_utf8_lossy(&body[2 + len..]).into()))
    }

    fn sent(wire: &Arc<Mutex<Wire>>, connection: usize) -> Vec<Vec<u8>> {
        wire.lock().unwrap().connections[connection].clone()
    }

    #[test]
    fn connect_announces_device_then_online_and_state() {
        let (mut s, wire) = session();
        let t = Instant::now();
        assert_eq!(s.step(Some(&cfg()), &live(&["zoom.us"]), false, t), Some(Ok(())));
        let packets = sent(&wire, 0);
        let topics: Vec<(String, String)> = packets.iter().filter_map(|p| topic_of(p)).collect();
        assert_eq!(topics.len(), 5);
        assert!(topics[0].0.ends_with("/on_air/config"));
        assert!(topics[2].0.ends_with("/program/config"));
        assert_eq!(topics[3], ("micled/a1b2c3d4e5f6/availability".into(), "online".into()));
        assert_eq!(topics[4].0, "micled/a1b2c3d4e5f6/state");
        assert!(topics[4].1.contains("\"program\":\"zoom.us\""));
    }

    #[test]
    fn state_only_on_change_and_ping_when_idle() {
        let (mut s, wire) = session();
        let t = Instant::now();
        s.step(Some(&cfg()), &live(&[]), false, t);
        assert_eq!(s.step(Some(&cfg()), &live(&[]), false, t + Duration::from_secs(1)), None, "nichts Neues");
        assert_eq!(sent(&wire, 0).len(), 5);
        assert_eq!(s.step(Some(&cfg()), &Report::default(), false, t + Duration::from_secs(2)), Some(Ok(())));
        assert_eq!(sent(&wire, 0).len(), 6, "neuer Zustand");
        assert_eq!(s.next_wake(Some(&cfg()), false, t), Some(t + Duration::from_secs(17)));
        assert_eq!(s.step(Some(&cfg()), &Report::default(), false, t + Duration::from_secs(17)), Some(Ok(())));
        assert_eq!(wire.lock().unwrap().pings, 1);
    }

    #[test]
    fn sleep_sends_offline_and_waits_for_wake() {
        let (mut s, wire) = session();
        let t = Instant::now();
        s.step(Some(&cfg()), &live(&[]), false, t);
        assert_eq!(s.step(Some(&cfg()), &live(&[]), true, t), Some(Ok(())));
        let packets = sent(&wire, 0);
        let n = packets.len();
        assert_eq!(topic_of(&packets[n - 2]), Some(("micled/a1b2c3d4e5f6/availability".into(), "offline".into())));
        assert_eq!(packets[n - 1], DISCONNECT);
        assert_eq!(s.next_wake(Some(&cfg()), true, t), None, "kein Neuversuch im Ruhezustand");
        s.step(Some(&cfg()), &live(&[]), true, t + Duration::from_secs(60));
        assert_eq!(wire.lock().unwrap().connections.len(), 1);
        assert_eq!(s.step(Some(&cfg()), &live(&[]), false, t + Duration::from_secs(61)), Some(Ok(())), "wach");
        assert_eq!(wire.lock().unwrap().connections.len(), 2);
    }

    #[test]
    fn will_is_offline_on_availability_topic() {
        let c = cfg();
        let topic = availability_topic(&c);
        let p = connect_packet("micled-a1b2c3d4e5f6", Some(&Will { topic: &topic, message: b"offline" }), "", "", KEEPALIVE_S);
        let (_, body) = read_packet(&mut Cursor::new(&p)).unwrap();
        assert_eq!(body[7] & 0x24, 0x24, "Will mit retain");
        assert_eq!(body[7] & 0x18, 0, "Will mit QoS 0");
        let tail = [&b"\x00\x20micled/a1b2c3d4e5f6/availability"[..], b"\x00\x07offline"].concat();
        assert!(body.ends_with(&tail));
    }

    #[test]
    fn switching_off_removes_the_device() {
        let (mut s, wire) = session();
        let t = Instant::now();
        s.step(Some(&cfg()), &live(&[]), false, t);
        let before = sent(&wire, 0).len();
        assert_eq!(s.step(None, &live(&[]), false, t), Some(Ok(())));
        let packets = sent(&wire, 0)[before..].to_vec();
        let cleared: Vec<(String, String)> = packets.iter().filter_map(|p| topic_of(p)).collect();
        assert_eq!(cleared.len(), 5);
        assert!(cleared.iter().all(|(_, m)| m.is_empty()), "leere retained Nachrichten");
        assert!(cleared.iter().any(|(t, _)| t == "micled/a1b2c3d4e5f6/state"));
        assert!(cleared.iter().any(|(t, _)| t == "micled/a1b2c3d4e5f6/availability"));
        assert_eq!(packets.last().unwrap(), &DISCONNECT.to_vec());
        assert_eq!(s.next_wake(None, false, t), None);
    }

    #[test]
    fn off_at_start_does_nothing() {
        let (mut s, wire) = session();
        assert_eq!(s.step(None, &Report::default(), false, Instant::now()), Some(Ok(())));
        assert!(wire.lock().unwrap().connections.is_empty());
    }

    #[test]
    fn other_settings_say_goodbye_first() {
        let (mut s, wire) = session();
        let t = Instant::now();
        s.step(Some(&cfg()), &live(&[]), false, t);
        let other = Mqtt { host: "10.0.0.2".into(), ..cfg() };
        assert_eq!(s.next_wake(Some(&other), false, t), Some(t));
        s.step(Some(&other), &live(&[]), false, t);
        let first = sent(&wire, 0);
        assert_eq!(first.last().unwrap(), &DISCONNECT.to_vec());
        assert_eq!(topic_of(&first[first.len() - 2]).unwrap().1, "offline");
        assert_eq!(sent(&wire, 1).len(), 5, "neu angemeldet");
    }

    #[test]
    fn errors_retry_every_ten_seconds() {
        let (mut s, wire) = session();
        let t = Instant::now();
        wire.lock().unwrap().refuse = true;
        assert!(s.step(Some(&cfg()), &live(&[]), false, t).unwrap().is_err());
        assert_eq!(s.step(Some(&cfg()), &live(&[]), false, t + Duration::from_secs(5)), None, "Pause");
        assert_eq!(s.next_wake(Some(&cfg()), false, t), Some(t + Duration::from_secs(10)));
        let other = Mqtt { port: 1884, ..cfg() };
        assert_eq!(s.next_wake(Some(&other), false, t), Some(t), "neue Einstellung sofort");
        wire.lock().unwrap().refuse = false;
        assert_eq!(s.step(Some(&cfg()), &live(&[]), false, t + Duration::from_secs(10)), Some(Ok(())));
    }

    #[test]
    fn lost_connection_reconnects() {
        let (mut s, wire) = session();
        let t = Instant::now();
        s.step(Some(&cfg()), &live(&[]), false, t);
        wire.lock().unwrap().broken = true;
        assert!(s.step(Some(&cfg()), &Report::default(), false, t).unwrap().is_err());
        assert_eq!(s.next_wake(Some(&cfg()), false, t), Some(t), "gleich neu verbinden");
        wire.lock().unwrap().broken = false;
        assert_eq!(s.step(Some(&cfg()), &Report::default(), false, t), Some(Ok(())));
        assert_eq!(wire.lock().unwrap().connections.len(), 2);
    }

    // --- Thread und echte Verbindung ----------------------------------------

    fn wait_for(slot: &Arc<Mutex<Option<String>>>, done: impl Fn(&Option<String>) -> bool) -> Option<String> {
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            let v = slot.lock().unwrap().clone();
            if done(&v) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        slot.lock().unwrap().clone()
    }

    /// Port ohne Broker
    fn closed_port() -> u16 {
        TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn send_returns_at_once_and_reports_unreachable_broker() {
        let error = Arc::new(Mutex::new(None));
        let u = Updater::start_with(error.clone(), net_connector(), FAST);
        let started = Instant::now();
        u.send(Some(Mqtt { port: closed_port(), ..cfg() }), live(&[]), false);
        assert!(started.elapsed() < Duration::from_millis(20), "send wartet nicht auf den Broker");
        let e = wait_for(&error, Option::is_some).expect("Fehler im Slot");
        assert!(e.starts_with("MQTT: Broker nicht erreichbar"), "{e}");
    }

    #[test]
    fn shutdown_is_acknowledged() {
        let error = Arc::new(Mutex::new(None));
        let (s, wire) = session();
        let u = Updater::start_with(error, s.connect, FAST);
        u.send(Some(cfg()), live(&[]), false);
        let until = Instant::now() + Duration::from_secs(5);
        while wire.lock().unwrap().connections.is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let done = u.begin_shutdown().expect("Thread läuft");
        done.recv_timeout(Duration::from_secs(3)).expect("bestätigt");
        let packets = sent(&wire, 0);
        assert_eq!(topic_of(&packets[packets.len() - 2]).unwrap().1, "offline");
        assert_eq!(packets.last().unwrap(), &DISCONNECT.to_vec());
    }

    /// Broker, der auf CONNECT mit `code` antwortet und das CONNECT zurückgibt
    fn broker(code: u8) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let (_, body) = read_packet(&mut s).unwrap();
            s.write_all(&[0x20, 2, 0, code]).unwrap();
            body
        });
        (port, h)
    }

    #[test]
    fn check_reports_login_result() {
        let (port, h) = broker(0);
        assert_eq!(check(&Mqtt { port, ..cfg() }), Ok(()));
        let connect = h.join().unwrap();
        assert!(String::from_utf8_lossy(&connect).contains("micled-check-"), "eigene Client-ID");
        let (port, _h) = broker(4);
        assert_eq!(check(&Mqtt { port, ..cfg() }).unwrap_err(), "MQTT: Benutzer oder Passwort falsch");
        let e = check(&Mqtt { port: closed_port(), ..cfg() }).unwrap_err();
        assert!(e.starts_with("MQTT: Broker nicht erreichbar"), "{e}");
    }
}
