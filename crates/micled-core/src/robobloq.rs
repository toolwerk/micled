//! Lichtstreifen hinter dem Monitor mit Robobloq-Controller (USB-HID
//! `1A86:FE07`, Steuer-App DX-Light), verkauft u. a. als Meewata oder
//! SyncLight. Der ganze Streifen zeigt die Farbe der [`Scene`], sonst ist er aus.
//!
//! Protokoll wie in DX-Static (MIT): Paket `'R' 'B' Länge Zähler Befehl
//! Daten… Prüfsumme` (unterstes Byte der Summe aller Bytes davor), als
//! 64-Byte-Output-Report mit Report-ID 0 an die Schnittstelle mit Usage Page
//! `0xFF00`. Befehle: `0x82` Geräteinfo, `0x86` Farbe je LED-Bereich, `0x87`
//! Helligkeit, `0x97` aus.
//!
//! Wie bei der Luxafor wird das Gerät nur für einen Befehl geöffnet und danach
//! wieder freigegeben. Steuern kann den Controller trotzdem nur ein Programm
//! zur Zeit, DX-Light und Co. müssen dafür beendet sein.

use std::ffi::CString;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::color::Rgb;
use crate::display::Scene;
use crate::output::{scene_color, Device};

pub const VENDOR_ID: u16 = 0x1a86;
pub const PRODUCT_ID: u16 = 0xfe07;
/// Steuer-Schnittstelle; die zweite (Tastatur) verlangt unter macOS
/// „Eingabeüberwachung“ und bleibt unberührt
pub const USAGE_PAGE: u16 = 0xff00;

const REPORT_LEN: usize = 64;
const ACT_INFO: u8 = 0x82;
const ACT_COLOR: u8 = 0x86;
const ACT_BRIGHTNESS: u8 = 0x87;
const ACT_OFF: u8 = 0x97;
/// Letzte LED eines Bereichs, wenn die Zahl der LEDs unbekannt ist
const ALL_LEDS: u8 = 254;
/// Pause nach dem Helligkeitsbefehl vor dem Farbrahmen (wie DX-Light)
const BRIGHTNESS_GAP: Duration = Duration::from_millis(220);
/// Ausschalten wird viermal gesendet, mit kurzer Pause (wie DX-Light)
const OFF_REPEAT: usize = 4;
const OFF_GAP: Duration = Duration::from_millis(20);
/// So oft wird auf die Antwort zur Geräteinfo gewartet, je 400 ms
const READ_TRIES: usize = 8;
const READ_TIMEOUT_MS: i32 = 400;

pub const NOT_FOUND: &str = "Kein Lichtstreifen mit USB 1A86:FE07 gefunden (angesteckt?)";

/// Zähler der Pakete; das Schloss hält außerdem zwei Vorgänge (Updater,
/// „Testen“) auseinander, damit Antworten nicht beim falschen landen
static USB: Mutex<u8> = Mutex::new(0);

/// Paket mit Kopf und Prüfsumme
pub fn packet(seq: u8, action: u8, payload: &[u8]) -> Vec<u8> {
    let mut p = vec![b'R', b'B', (6 + payload.len()) as u8, seq, action];
    p.extend_from_slice(payload);
    let sum = p.iter().fold(0u8, |s, b| s.wrapping_add(*b));
    p.push(sum);
    p
}

/// Nächster Zähler: 1 bis 254, danach wieder 1
pub fn next_seq(prev: u8) -> u8 {
    if prev >= 254 {
        1
    } else {
        prev + 1
    }
}

/// Begrenzt die Summe von Rot, Grün und Blau auf 255 wie DX-Light, damit der
/// USB-Anschluss nicht überlastet
fn limit(c: Rgb) -> Rgb {
    let sum = c.r as u32 + c.g as u32 + c.b as u32;
    if sum <= 255 {
        return c;
    }
    let k = sum as f64 / 255.0;
    let f = |v: u8| (v as f64 / k) as u8;
    Rgb { r: f(c.r), g: f(c.g), b: f(c.b) }
}

/// Daten des Farbbefehls: alle LEDs in der Farbe, bei bekannter Zahl der LEDs
/// der Rest bis 254 schwarz
fn color_frame(c: Rgb, leds: Option<u8>) -> Vec<u8> {
    let c = limit(c);
    match leds.filter(|n| (1..=253).contains(n)) {
        Some(n) => vec![1, c.r, c.g, c.b, n, n + 1, 0, 0, 0, ALL_LEDS],
        None => vec![1, c.r, c.g, c.b, ALL_LEDS],
    }
}

/// Antwort auf die Geräteinfo
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    /// Monitorgröße in Zoll
    pub inches: u8,
    pub leds: u8,
    pub firmware: [u8; 3],
}

impl Info {
    /// Antwort ohne Report-ID (so liefert sie hidapi), passend zum Zähler `seq`
    pub fn parse(reply: &[u8], seq: u8) -> Option<Info> {
        if reply.len() < 24 || &reply[0..2] != b"RB" || reply[3] != seq || reply[4] != ACT_INFO {
            return None;
        }
        Some(Info { inches: reply[8], leds: reply[11], firmware: [reply[21], reply[22], reply[23]] })
    }

    /// Firmware als „1.9.4“
    pub fn firmware_text(&self) -> String {
        let [a, b, c] = self.firmware;
        format!("{a}.{b}.{c}")
    }
}

/// Befehle zum Ausschalten: ab Firmware 1.8.0 (oder unbekannt) `0x97`, sonst
/// ein schwarzer Farbrahmen
fn off_command(info: Option<&Info>) -> (u8, Vec<u8>) {
    match info {
        Some(i) if i.firmware < [1, 8, 0] => (ACT_COLOR, color_frame(Rgb::OFF, Some(i.leds))),
        _ => (ACT_OFF, Vec::new()),
    }
}

/// Steuer-Schnittstellen aus den USB-Geräten mit der Hersteller-ID von
/// Robobloq (Produkt-ID, Usage Page, Pfad). Kein passender Streifen ist ein Fehler.
fn choose<T: PartialEq>(devices: Vec<(u16, u16, T)>) -> Result<Vec<T>, String> {
    let mut out: Vec<T> = Vec::new();
    let mut other = None;
    for (pid, usage, path) in devices {
        if usage != USAGE_PAGE {
            continue;
        }
        if pid != PRODUCT_ID {
            other.get_or_insert(pid);
        } else if !out.contains(&path) {
            out.push(path);
        }
    }
    match (out.is_empty(), other) {
        (false, _) => Ok(out),
        (true, Some(pid)) => Err(format!("Controller 1A86:{pid:04X} wird nicht unterstützt (nur 1A86:FE07)")),
        (true, None) => Err(NOT_FOUND.into()),
    }
}

fn find_in(api: &HidApi) -> Result<Vec<CString>, String> {
    let devices = api
        .device_list()
        .filter(|d| d.vendor_id() == VENDOR_ID)
        .map(|d| (d.product_id(), d.usage_page(), d.path().to_owned()))
        .collect();
    choose(devices)
}

fn usb_error(e: impl std::fmt::Display) -> String {
    format!("Lichtstreifen per USB: {e}")
}

/// Angesteckte Streifen (Pfade der Steuer-Schnittstelle)
pub fn find() -> Result<Vec<CString>, String> {
    find_in(&HidApi::new().map_err(usb_error)?)
}

/// Ein Befehl an alle Streifen, mit dem Gerät nur für die Dauer des Befehls offen
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// Geräteinfo des ersten Streifens lesen
    ReadInfo,
    /// Volle Helligkeit (gedimmt wird über die Farbe)
    Brightness,
    /// Ganzer Streifen in der Farbe, mit Zahl der LEDs, falls bekannt
    Color(Rgb, Option<u8>),
    Off(Option<Info>),
}

fn write(dev: &HidDevice, seq: &mut u8, action: u8, payload: &[u8]) -> Result<u8, String> {
    *seq = next_seq(*seq);
    let p = packet(*seq, action, payload);
    let mut report = [0u8; REPORT_LEN + 1];
    report[1..1 + p.len()].copy_from_slice(&p);
    dev.write(&report).map_err(usb_error)?;
    Ok(*seq)
}

fn read_info(dev: &HidDevice, seq: &mut u8) -> Result<Option<Info>, String> {
    let sent = write(dev, seq, ACT_INFO, &[])?;
    let mut buf = [0u8; REPORT_LEN];
    for _ in 0..READ_TRIES {
        let n = dev.read_timeout(&mut buf, READ_TIMEOUT_MS).map_err(usb_error)?;
        if let Some(info) = Info::parse(&buf[..n], sent) {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

/// Führt den Befehl auf jedem gefundenen Streifen aus. `ReadInfo` liefert die
/// Info des ersten, der antwortet (`None` ohne Antwort).
pub fn execute(cmd: Cmd) -> Result<Option<Info>, String> {
    let mut seq = USB.lock().unwrap_or_else(|e| e.into_inner());
    let api = HidApi::new().map_err(usb_error)?;
    let mut info = None;
    for path in find_in(&api)? {
        let dev = api.open_path(&path).map_err(usb_error)?;
        match cmd {
            Cmd::ReadInfo => {
                info = read_info(&dev, &mut seq)?;
                if info.is_some() {
                    break;
                }
            }
            Cmd::Brightness => {
                write(&dev, &mut seq, ACT_BRIGHTNESS, &[255])?;
            }
            Cmd::Color(c, leds) => {
                write(&dev, &mut seq, ACT_COLOR, &color_frame(c, leds))?;
            }
            Cmd::Off(i) => {
                let (action, payload) = off_command(i.as_ref());
                for n in 0..OFF_REPEAT {
                    if n > 0 {
                        sleep(OFF_GAP);
                    }
                    write(&dev, &mut seq, action, &payload)?;
                }
            }
        }
        // `dev` wird hier geschlossen, andere Programme bleiben unberührt
    }
    if cmd == Cmd::Brightness {
        sleep(BRIGHTNESS_GAP);
    }
    Ok(info)
}

/// Geräteinfo des ersten Streifens (für „Testen“ und `micled strip info`)
pub fn check() -> Result<Info, String> {
    execute(Cmd::ReadInfo)?.ok_or_else(|| "Lichtstreifen antwortet nicht (läuft DX-Light oder ein anderes Programm?)".into())
}

/// Ziel in den Einstellungen. `connection` zählt das Anstecken: Ein neuer Wert
/// heißt, der Streifen war weg und bekommt alles neu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Strip {
    pub connection: u32,
}

/// Merkt, ob ein Streifen angesteckt ist, und zählt das Wiederanstecken
#[derive(Debug, Default)]
pub struct Watch {
    pub present: Option<bool>,
    pub connection: u32,
}

impl Watch {
    /// Ergebnis einer Suche; `false → true` ist ein neues Anstecken
    pub fn seen(&mut self, present: bool) {
        if self.present == Some(false) && present {
            self.connection += 1;
        }
        self.present = Some(present);
    }
}

/// Was MicLED dem Streifen seit dem Anstecken geschickt hat
#[derive(Debug, Default)]
pub struct Session {
    cfg: Option<Strip>,
    /// `None` = noch nicht gelesen, `Some(None)` = keine Antwort
    info: Option<Option<Info>>,
    bright: bool,
    /// Zuletzt sicher gezeigte Farbe
    shown: Option<Rgb>,
    /// Eine Farbe von MicLED ist womöglich zu sehen (auch nach einem Fehler)
    lit: bool,
    /// Seit dem Anstecken ausgeschaltet und seither nichts gezeigt
    off: bool,
}

type SendFn<'a> = dyn FnMut(Cmd) -> Result<Option<Info>, String> + 'a;

impl Device for Session {
    type Cfg = Strip;

    fn apply(&mut self, cfg: Option<&Strip>, scene: &Scene) -> Result<(), String> {
        self.apply_with(cfg, scene, &mut execute)
    }

    fn restore(&mut self) -> Result<(), String> {
        if self.lit {
            self.turn_off(&mut execute)?;
        }
        Ok(())
    }

    fn current(&self) -> Option<&Strip> {
        self.cfg.as_ref()
    }
}

impl Session {
    fn apply_with(&mut self, cfg: Option<&Strip>, scene: &Scene, send: &mut SendFn<'_>) -> Result<(), String> {
        let Some(strip) = cfg else {
            // ausgeschaltet: eine gezeigte Farbe geht aus, sonst bleibt alles
            if self.lit {
                self.turn_off(send)?;
            }
            *self = Session::default();
            return Ok(());
        };
        if self.cfg.as_ref() != Some(strip) {
            // neu angesteckt: Zustand gilt nicht mehr, ausschalten ginge ins Leere
            *self = Session { cfg: Some(strip.clone()), ..Session::default() };
        }
        let Some(color) = scene_color(scene) else {
            return if self.off { Ok(()) } else { self.turn_off(send) };
        };
        if self.shown == Some(color) {
            return Ok(());
        }
        let info = self.info(send)?;
        if !self.bright {
            send(Cmd::Brightness)?;
            self.bright = true;
        }
        self.lit = true;
        self.off = false;
        self.shown = None;
        send(Cmd::Color(color, info.map(|i| i.leds)))?;
        self.shown = Some(color);
        Ok(())
    }

    fn info(&mut self, send: &mut SendFn<'_>) -> Result<Option<Info>, String> {
        if let Some(i) = self.info {
            return Ok(i);
        }
        let i = send(Cmd::ReadInfo)?;
        self.info = Some(i);
        Ok(i)
    }

    fn turn_off(&mut self, send: &mut SendFn<'_>) -> Result<(), String> {
        let info = self.info(send)?;
        send(Cmd::Off(info))?;
        self.lit = false;
        self.off = true;
        self.shown = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Antwort des Meewata 32" (10.11.2026), ohne Report-ID
    const REPLY: [u8; 24] = [
        0x52, 0x42, 0x19, 0x01, 0x82, 0x00, 0x06, 0x0c, 0x20, 0x01, 0x00, 0x4e, 0xcd, 0xab, 0x29, 0x4e, 0x2d, 0xbd, 0x23, 0xb7,
        0xe5, 0x01, 0x09, 0x04,
    ];

    const MEEWATA: Info = Info { inches: 32, leds: 78, firmware: [1, 9, 4] };
    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED };

    #[test]
    fn info_request() {
        assert_eq!(packet(1, ACT_INFO, &[]), [0x52, 0x42, 0x06, 0x01, 0x82, 0x1d]);
        let p = packet(200, ACT_BRIGHTNESS, &[255]);
        assert_eq!(p[2], 7);
        assert_eq!(*p.last().unwrap(), p[..6].iter().fold(0u8, |s, b| s.wrapping_add(*b)), "Prüfsumme läuft über");
    }

    #[test]
    fn reads_device_info() {
        let mut reply = REPLY.to_vec();
        reply.resize(64, 0);
        let info = Info::parse(&reply, 1).unwrap();
        assert_eq!(info, MEEWATA);
        assert_eq!(info.firmware_text(), "1.9.4");
        assert_eq!(Info::parse(&reply, 2), None, "anderer Zähler");
        reply[4] = ACT_COLOR;
        assert_eq!(Info::parse(&reply, 1), None, "andere Aktion");
        assert_eq!(Info::parse(&REPLY[..20], 1), None, "zu kurz");
    }

    #[test]
    fn color_frames() {
        let c = Rgb { r: 200, g: 0, b: 30 };
        assert_eq!(color_frame(c, Some(78)), [1, 200, 0, 30, 78, 79, 0, 0, 0, 254]);
        assert_eq!(color_frame(c, None), [1, 200, 0, 30, 254]);
        assert_eq!(color_frame(c, Some(0)), [1, 200, 0, 30, 254], "unbrauchbare Zahl: alle");
        assert_eq!(color_frame(c, Some(254)), [1, 200, 0, 30, 254]);
    }

    #[test]
    fn bright_colors_are_limited() {
        assert_eq!(limit(Rgb { r: 255, g: 255, b: 255 }), Rgb { r: 85, g: 85, b: 85 });
        assert_eq!(limit(Rgb { r: 255, g: 136, b: 0 }), Rgb { r: 166, g: 88, b: 0 });
        assert_eq!(limit(Rgb::RED), Rgb::RED);
    }

    #[test]
    fn counter_wraps_to_one() {
        assert_eq!(next_seq(0), 1);
        assert_eq!(next_seq(1), 2);
        assert_eq!(next_seq(253), 254);
        assert_eq!(next_seq(254), 1);
        assert_eq!(next_seq(255), 1);
    }

    #[test]
    fn off_depends_on_firmware() {
        assert_eq!(off_command(None), (ACT_OFF, vec![]));
        assert_eq!(off_command(Some(&MEEWATA)), (ACT_OFF, vec![]));
        let old = Info { firmware: [1, 7, 9], ..MEEWATA };
        assert_eq!(off_command(Some(&old)), (ACT_COLOR, vec![1, 0, 0, 0, 78, 79, 0, 0, 0, 254]));
    }

    #[test]
    fn only_the_supported_controller() {
        let found = choose(vec![(PRODUCT_ID, 0x01, "tastatur"), (PRODUCT_ID, USAGE_PAGE, "a"), (PRODUCT_ID, USAGE_PAGE, "a")]);
        assert_eq!(found, Ok(vec!["a"]), "nur die Steuerung, doppelte einmal");
        let other = choose(vec![(0xfe0c, USAGE_PAGE, "x")]);
        assert_eq!(other, Err("Controller 1A86:FE0C wird nicht unterstützt (nur 1A86:FE07)".into()));
        assert_eq!(choose::<&str>(vec![(PRODUCT_ID, 0x01, "tastatur")]), Err(NOT_FOUND.into()));
        assert_eq!(choose::<&str>(vec![]), Err(NOT_FOUND.into()));
    }

    #[test]
    fn plugging_in_again_counts() {
        let mut w = Watch::default();
        w.seen(true);
        assert_eq!(w.connection, 0, "erster Blick ist kein Anstecken");
        w.seen(true);
        w.seen(false);
        assert_eq!(w.connection, 0);
        w.seen(true);
        assert_eq!(w.connection, 1);
        let mut w = Watch::default();
        w.seen(false);
        w.seen(true);
        assert_eq!(w.connection, 1, "beim Start fehlend, dann angesteckt");
    }

    /// Streifen, der Befehle mitschreibt
    #[derive(Default)]
    struct Fake {
        log: Vec<Cmd>,
        fail: bool,
        info: Option<Info>,
    }

    impl Fake {
        fn new() -> Fake {
            Fake { info: Some(MEEWATA), ..Fake::default() }
        }

        fn send(&mut self) -> impl FnMut(Cmd) -> Result<Option<Info>, String> + '_ {
            move |cmd| {
                if self.fail {
                    return Err(NOT_FOUND.into());
                }
                self.log.push(cmd);
                Ok(if cmd == Cmd::ReadInfo { self.info } else { None })
            }
        }

        fn take(&mut self) -> Vec<Cmd> {
            std::mem::take(&mut self.log)
        }
    }

    fn strip(connection: u32) -> Strip {
        Strip { connection }
    }

    #[test]
    fn idle_turns_off_once_even_at_start() {
        let mut f = Fake::new();
        let mut s = Session::default();
        s.apply_with(Some(&strip(0)), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::ReadInfo, Cmd::Off(Some(MEEWATA))]);
        s.apply_with(Some(&strip(0)), &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty());
    }

    #[test]
    fn whole_strip_shows_the_color_then_goes_off() {
        let mut f = Fake::new();
        let mut s = Session::default();
        s.apply_with(Some(&strip(0)), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::ReadInfo, Cmd::Brightness, Cmd::Color(Rgb::RED, Some(78))]);
        s.apply_with(Some(&strip(0)), &Scene::Timer { minutes: 3, color: Rgb::RED }, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "gleiche Farbe nicht neu senden");
        s.apply_with(Some(&strip(0)), &Scene::Focus { color: Rgb::GREEN }, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::Color(Rgb::GREEN, Some(78))], "Info und Helligkeit nur einmal");
        s.apply_with(Some(&strip(0)), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::Off(Some(MEEWATA))]);
    }

    #[test]
    fn without_info_the_full_range_is_used() {
        let mut f = Fake::default();
        let mut s = Session::default();
        s.apply_with(Some(&strip(0)), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::ReadInfo, Cmd::Brightness, Cmd::Color(Rgb::RED, None)]);
    }

    #[test]
    fn plugged_in_again_gets_everything_anew() {
        let mut f = Fake::new();
        let mut s = Session::default();
        s.apply_with(Some(&strip(0)), &LIVE, &mut f.send()).unwrap();
        f.take();
        s.apply_with(Some(&strip(1)), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::ReadInfo, Cmd::Brightness, Cmd::Color(Rgb::RED, Some(78))], "ohne vorher auszuschalten");
        s.apply_with(Some(&strip(2)), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::ReadInfo, Cmd::Off(Some(MEEWATA))]);
    }

    #[test]
    fn switching_the_strip_off_in_the_settings() {
        let mut f = Fake::new();
        let mut s = Session::default();
        s.apply_with(Some(&strip(0)), &LIVE, &mut f.send()).unwrap();
        f.take();
        f.fail = true;
        assert!(s.apply_with(None, &LIVE, &mut f.send()).is_err());
        assert_eq!(s.current(), Some(&strip(0)), "Zustand bleibt für den nächsten Versuch");
        f.fail = false;
        s.apply_with(None, &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::Off(Some(MEEWATA))]);
        assert_eq!(s.current(), None);
        s.apply_with(None, &Scene::Idle, &mut f.send()).unwrap();
        assert!(f.take().is_empty(), "nichts gezeigt, nichts auszuschalten");
    }

    #[test]
    fn errors_keep_the_state_for_a_retry() {
        let mut f = Fake::new();
        f.fail = true;
        let mut s = Session::default();
        assert_eq!(s.apply_with(Some(&strip(0)), &LIVE, &mut f.send()).unwrap_err(), NOT_FOUND);
        f.fail = false;
        s.apply_with(Some(&strip(0)), &LIVE, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::ReadInfo, Cmd::Brightness, Cmd::Color(Rgb::RED, Some(78))], "wiederholt");
        f.fail = true;
        assert!(s.apply_with(Some(&strip(0)), &Scene::Idle, &mut f.send()).is_err());
        f.fail = false;
        s.apply_with(Some(&strip(0)), &Scene::Idle, &mut f.send()).unwrap();
        assert_eq!(f.take(), [Cmd::Off(Some(MEEWATA))], "Ausschalten wiederholt");
    }

    #[test]
    fn failed_color_still_counts_as_lit() {
        let mut s = Session::default();
        let mut flaky = |cmd: Cmd| {
            match cmd {
                Cmd::Color(..) => Err("weg".to_string()),
                Cmd::ReadInfo => Ok(Some(MEEWATA)),
                _ => Ok(None),
            }
        };
        assert!(s.apply_with(Some(&strip(0)), &LIVE, &mut flaky).is_err());
        assert!(s.lit, "die Farbe kann trotzdem angekommen sein");
        assert_eq!(s.shown, None);
    }
}
