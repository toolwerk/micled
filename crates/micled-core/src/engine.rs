//! Entscheidungslogik ohne Seiteneffekte: Mikro oder Kamera an → Mikro- bzw.
//! Kamerafarbe, danach vorherige oder feste Farbe. Gut testbar, weil Zeit,
//! Mikrofon- und Kamerazustand von außen hineingereicht werden.

use std::time::{Duration, Instant};

use crate::color::Rgb;
use crate::lamp::{Frame, Transport};
use crate::mic::MicUser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterMic {
    /// Zur Farbe zurück, die vor dem Mikrofon galt
    Restore,
    /// Immer auf die „Mikro aus“-Farbe
    Fixed,
}

/// Wo die Mikro- bzw. Kamerafarbe leuchtet. Die Luxafor Flag hat zwei Seiten
/// mit je drei LEDs; die andere Seite behält dann die ruhende Farbe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Both,
    /// LEDs 1–3
    A,
    /// LEDs 4–6
    B,
}

/// Gründe, aus denen niemand vor dem Mac sitzt. Mehrere können gleichzeitig
/// gelten (gesperrt und Bildschirm aus); zurück ist erst, wenn keiner mehr gilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Away {
    /// Ruhezustand des Macs
    Sleep,
    /// Bildschirm ausgeschaltet
    DisplaySleep,
    /// Bildschirm gesperrt
    Locked,
}

impl Away {
    fn bit(self) -> u8 {
        match self {
            Away::Sleep => 1,
            Away::DisplaySleep => 2,
            Away::Locked => 4,
        }
    }
}

/// Hintergrunddienste, die das Mikrofon dauerhaft offen halten, ohne dass
/// jemand spricht. Ab Werk ignoriert, in den Einstellungen entfernbar.
pub const DEFAULT_IGNORED: &[&str] = &[
    // Rogue Amoeba (SoundSource, Loopback, Audio Hijack)
    "com.rogueamoeba.arkaudiod",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Automatik an/aus
    pub enabled: bool,
    pub transport: Transport,
    pub mic_on_color: Rgb,
    /// Ziel bei `AfterMic::Fixed` und Rückfall, wenn keine vorherige Farbe bekannt ist
    pub mic_off_color: Rgb,
    pub after_mic: AfterMic,
    /// So lange muss das Mikro aus sein, bevor zurückgeschaltet wird
    pub release_delay: Duration,
    /// Bundle-IDs oder Prozessnamen, die nicht als „Mikro an“ zählen
    pub ignored: Vec<String>,
    /// Lampe aus bei Ruhezustand, Bildschirm aus und Sperre (außer während eines Calls)
    pub off_when_away: bool,
    /// Kamera auswerten: Kamera an zählt als aktiv und hat eine eigene Farbe
    pub camera: bool,
    pub camera_color: Rgb,
    pub side: Side,
    /// Ziel für „Auf Sendung“-Meldungen (leer = aus), siehe [`crate::onair`]
    pub on_air_url: String,
    /// Farbe der Vorwarnung vor Terminen
    pub reminder_color: Rgb,
    /// So lange vor Terminbeginn wird gewarnt
    pub reminder_lead: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            transport: Transport::Usb,
            mic_on_color: Rgb::RED,
            mic_off_color: Rgb::GREEN,
            after_mic: AfterMic::Restore,
            release_delay: Duration::from_secs(3),
            ignored: DEFAULT_IGNORED.iter().map(|s| s.to_string()).collect(),
            off_when_away: true,
            camera: true,
            camera_color: Rgb::MAGENTA,
            side: Side::Both,
            on_air_url: String::new(),
            reminder_color: Rgb::YELLOW,
            reminder_lead: Duration::from_secs(120),
        }
    }
}

#[derive(Debug)]
pub struct Engine {
    cfg: Config,
    /// Die „ruhende“ Farbe, zu der nach dem Mikrofon zurückgekehrt wird
    base: Option<Rgb>,
    overriding: bool,
    /// Zuletzt ausgegebene Mikro- bzw. Kamerafarbe
    override_shown: Option<Rgb>,
    last_active: Option<Instant>,
    users: Vec<MicUser>,
    camera: bool,
    /// Aktiv, einschließlich der Wartezeit nach dem Ende (unabhängig von der Automatik)
    on_air: bool,
    /// Gesetzte `Away`-Bits
    away: u8,
}

impl Engine {
    pub fn new(cfg: Config, base: Option<Rgb>) -> Self {
        Engine {
            cfg,
            base,
            overriding: false,
            override_shown: None,
            last_active: None,
            users: Vec::new(),
            camera: false,
            on_air: false,
            away: 0,
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }
    pub fn base(&self) -> Option<Rgb> {
        self.base
    }
    pub fn overriding(&self) -> bool {
        self.overriding
    }
    /// Mikrofon-Nutzer nach Abzug der ignorierten Programme
    pub fn users(&self) -> &[MicUser] {
        &self.users
    }
    /// Kamera in Benutzung (nur, wenn die Kamera ausgewertet wird)
    pub fn camera_on(&self) -> bool {
        self.camera
    }

    /// Auf Sendung: Mikro oder Kamera aktiv, kurze Pausen eingeschlossen.
    /// Im Ruhezustand nie, sonst zählt ein vor dem Zuklappen laufender Call weiter.
    pub fn live(&self) -> bool {
        self.on_air && self.away & Away::Sleep.bit() == 0
    }

    fn is_ignored(&self, u: &MicUser) -> bool {
        self.cfg.ignored.iter().any(|i| {
            let i = i.trim();
            !i.is_empty() && (i.eq_ignore_ascii_case(&u.bundle_id) || i.eq_ignore_ascii_case(&u.name))
        })
    }

    /// Lampe soll wegen Abwesenheit aus sein
    pub fn dark(&self) -> bool {
        self.cfg.off_when_away && self.away != 0
    }

    /// Farbe, wenn weder Mikrofon noch Kamera aktiv sind
    fn resting(&self) -> Rgb {
        if self.dark() {
            Rgb::OFF
        } else {
            self.base.unwrap_or(self.cfg.mic_off_color)
        }
    }

    fn override_color(&self) -> Rgb {
        if self.camera {
            self.cfg.camera_color
        } else {
            self.cfg.mic_on_color
        }
    }

    /// Was die Lampe für die ausgegebene Farbe `c` zeigen soll. Nur die Mikro-
    /// bzw. Kamerafarbe kann auf eine Seite beschränkt sein, und nur per USB.
    pub fn frame(&self, c: Rgb) -> Frame {
        let split = self.overriding && self.override_shown == Some(c) && self.cfg.transport == Transport::Usb;
        match (split, self.cfg.side) {
            (true, Side::A) => Frame { a: c, b: self.resting() },
            (true, Side::B) => Frame { a: self.resting(), b: c },
            _ => Frame::solid(c),
        }
    }

    /// Abwesenheit beginnt (`on = true`) oder endet. Während eines Calls bleibt
    /// die Mikrofarbe; endet er in der Abwesenheit, geht die Lampe aus.
    pub fn set_away(&mut self, reason: Away, on: bool) -> Option<Rgb> {
        let was = self.dark();
        if on {
            self.away |= reason.bit();
        } else {
            self.away &= !reason.bit();
        }
        if was == self.dark() {
            return None;
        }
        if self.overriding {
            // ruhende Seite einer geteilten Anzeige mitziehen
            return (self.cfg.side != Side::Both).then(|| self.override_color());
        }
        Some(self.resting())
    }

    fn restore_color(&self) -> Rgb {
        match self.cfg.after_mic {
            AfterMic::Restore => self.base.unwrap_or(self.cfg.mic_off_color),
            AfterMic::Fixed => self.cfg.mic_off_color,
        }
    }

    /// Neuer Mikrofon-Zustand ohne Kamera (Kurzform von [`Engine::on_input`])
    pub fn on_mic(&mut self, users: Vec<MicUser>, now: Instant) -> Option<Rgb> {
        self.on_input(users, false, now)
    }

    /// Neuer Zustand von Mikrofon und Kamera. Liefert eine Farbe, wenn die
    /// Lampe umgeschaltet werden soll.
    pub fn on_input(&mut self, users: Vec<MicUser>, camera: bool, now: Instant) -> Option<Rgb> {
        self.users = users.into_iter().filter(|u| !self.is_ignored(u)).collect();
        self.camera = camera && self.cfg.camera;
        let active = !self.users.is_empty() || self.camera;

        if active {
            self.last_active = Some(now);
        }
        let since = self.last_active.map(|t| now.duration_since(t)).unwrap_or(Duration::MAX);
        self.on_air = active || since < self.cfg.release_delay;

        if active {
            if self.cfg.enabled && (!self.overriding || self.override_shown != Some(self.override_color())) {
                return Some(self.start_override());
            }
            return None;
        }

        if self.overriding && since >= self.cfg.release_delay {
            return Some(self.end_override());
        }
        None
    }

    fn start_override(&mut self) -> Rgb {
        self.overriding = true;
        let c = self.override_color();
        self.override_shown = Some(c);
        c
    }

    fn end_override(&mut self) -> Rgb {
        self.overriding = false;
        self.override_shown = None;
        let c = self.restore_color();
        self.base = Some(c);
        self.resting()
    }

    /// Farbe direkt in MicLED gewählt. Gilt sofort, auch während des Mikrofons,
    /// und wird zur neuen ruhenden Farbe.
    pub fn on_user_color(&mut self, c: Rgb) -> Rgb {
        self.base = Some(c);
        c
    }

    /// Ein anderes Programm (Stream Deck) hat die Lampe gesetzt.
    pub fn on_shared_color(&mut self, c: Rgb) {
        self.base = Some(c);
    }

    /// Einstellungen ändern. Liefert eine Farbe, falls das sofort sichtbar werden muss.
    pub fn set_config(&mut self, cfg: Config) -> Option<Rgb> {
        let was_dark = self.dark();
        let old = std::mem::replace(&mut self.cfg, cfg);
        if !self.cfg.camera {
            self.camera = false;
        }
        if !self.overriding {
            return (was_dark != self.dark()).then(|| self.resting());
        }
        if !self.cfg.enabled {
            return Some(self.end_override());
        }
        if self.override_shown != Some(self.override_color()) || old.side != self.cfg.side || was_dark != self.dark() {
            return Some(self.start_override());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str) -> MicUser {
        MicUser { pid: 1, bundle_id: format!("com.example.{name}"), name: name.into() }
    }

    fn engine(after: AfterMic) -> (Engine, Instant) {
        let cfg = Config { after_mic: after, ..Config::default() };
        (Engine::new(cfg, Some(Rgb::parse("0000ff").unwrap())), Instant::now())
    }

    #[test]
    fn mic_on_switches_to_mic_color_once() {
        let (mut e, t) = engine(AfterMic::Restore);
        assert_eq!(e.on_mic(vec![user("zoom")], t), Some(Rgb::RED));
        assert_eq!(e.on_mic(vec![user("zoom")], t + Duration::from_secs(1)), None);
        assert!(e.overriding());
    }

    #[test]
    fn restores_previous_color_after_delay() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(1)), None, "Verzögerung abwarten");
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(4)), Rgb::parse("0000ff"));
        assert!(!e.overriding());
    }

    #[test]
    fn short_gap_does_not_flicker() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        e.on_mic(vec![], t + Duration::from_secs(1));
        assert_eq!(e.on_mic(vec![user("zoom")], t + Duration::from_secs(2)), None);
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(4)), None, "Zähler läuft neu ab letztem Aktiv");
    }

    #[test]
    fn fixed_mode_uses_mic_off_color() {
        let (mut e, t) = engine(AfterMic::Fixed);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(5)), Some(Rgb::GREEN));
    }

    #[test]
    fn unknown_base_falls_back_to_mic_off_color() {
        let mut e = Engine::new(Config::default(), None);
        let t = Instant::now();
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(5)), Some(Rgb::GREEN));
    }

    #[test]
    fn color_changed_during_meeting_is_restored_later() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        e.on_shared_color(Rgb::parse("ffff00").unwrap());
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(5)), Rgb::parse("ffff00"));
    }

    fn daemon() -> MicUser {
        MicUser { pid: 1, bundle_id: "com.rogueamoeba.arkaudiod".into(), name: "arkaudiod".into() }
    }

    #[test]
    fn default_config_ignores_audio_daemons() {
        let mut e = Engine::new(Config::default(), None);
        assert_eq!(e.on_mic(vec![daemon()], Instant::now()), None);
        assert!(e.users().is_empty());
    }

    #[test]
    fn audio_daemon_counts_when_removed_from_ignored() {
        let cfg = Config { ignored: vec![], ..Config::default() };
        let mut e = Engine::new(cfg, None);
        assert_eq!(e.on_mic(vec![daemon()], Instant::now()), Some(Rgb::RED));
    }

    #[test]
    fn ignored_apps_do_not_count() {
        let cfg = Config { ignored: vec!["com.example.plaud".into()], ..Config::default() };
        let mut e = Engine::new(cfg, None);
        assert_eq!(e.on_mic(vec![user("plaud")], Instant::now()), None);
        assert!(e.users().is_empty());
    }

    #[test]
    fn disabled_does_not_switch_but_reports_users() {
        let cfg = Config { enabled: false, ..Config::default() };
        let mut e = Engine::new(cfg, None);
        assert_eq!(e.on_mic(vec![user("zoom")], Instant::now()), None);
        assert_eq!(e.users().len(), 1);
    }

    #[test]
    fn disabling_during_meeting_restores() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        let cfg = Config { enabled: false, ..e.config().clone() };
        assert_eq!(e.set_config(cfg), Rgb::parse("0000ff"));
    }

    #[test]
    fn new_mic_color_applies_immediately_while_active() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        let yellow = Rgb::parse("ffcc00").unwrap();
        let cfg = Config { mic_on_color: yellow, ..e.config().clone() };
        assert_eq!(e.set_config(cfg), Some(yellow));
    }

    fn blue() -> Rgb {
        Rgb::parse("0000ff").unwrap()
    }

    #[test]
    fn away_turns_lamp_off_and_back_restores_resting_color() {
        let (mut e, _) = engine(AfterMic::Restore);
        assert_eq!(e.set_away(Away::Locked, true), Some(Rgb::OFF));
        assert_eq!(e.set_away(Away::Locked, false), Some(blue()));
    }

    #[test]
    fn repeated_away_signal_switches_once() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.set_away(Away::Locked, true);
        assert_eq!(e.set_away(Away::Locked, true), None);
    }

    #[test]
    fn back_only_when_all_away_reasons_are_gone() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.set_away(Away::Locked, true);
        assert_eq!(e.set_away(Away::DisplaySleep, true), None);
        assert_eq!(e.set_away(Away::DisplaySleep, false), None, "noch gesperrt");
        assert_eq!(e.set_away(Away::Locked, false), Some(blue()));
    }

    #[test]
    fn locking_during_call_keeps_mic_color() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.set_away(Away::Locked, true), None);
    }

    #[test]
    fn call_ending_while_away_turns_lamp_off_and_back_restores() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        e.set_away(Away::Locked, true);
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(5)), Some(Rgb::OFF));
        assert_eq!(e.set_away(Away::Locked, false), Some(blue()));
    }

    #[test]
    fn call_while_away_shows_mic_color() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.set_away(Away::Locked, true);
        assert_eq!(e.on_mic(vec![user("zoom")], t), Some(Rgb::RED));
    }

    #[test]
    fn back_during_call_keeps_mic_color() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.set_away(Away::Locked, true);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.set_away(Away::Locked, false), None);
    }

    #[test]
    fn away_with_unknown_color_restores_mic_off_color() {
        let mut e = Engine::new(Config::default(), None);
        e.set_away(Away::Sleep, true);
        assert_eq!(e.set_away(Away::Sleep, false), Some(Rgb::GREEN));
    }

    #[test]
    fn away_does_nothing_when_switched_off() {
        let cfg = Config { off_when_away: false, ..Config::default() };
        let mut e = Engine::new(cfg, Some(blue()));
        assert_eq!(e.set_away(Away::Locked, true), None);
        assert_eq!(e.set_away(Away::Locked, false), None);
    }

    #[test]
    fn switching_setting_while_away_applies_immediately() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.set_away(Away::Locked, true);
        let off = Config { off_when_away: false, ..e.config().clone() };
        assert_eq!(e.set_config(off), Some(blue()));
        let on = Config { off_when_away: true, ..e.config().clone() };
        assert_eq!(e.set_config(on), Some(Rgb::OFF));
    }

    #[test]
    fn color_set_while_away_is_restored_on_return() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.set_away(Away::Locked, true);
        let yellow = Rgb::parse("ffff00").unwrap();
        e.on_shared_color(yellow);
        assert_eq!(e.set_away(Away::Locked, false), Some(yellow));
    }

    // --- Kamera ---------------------------------------------------------

    fn magenta() -> Rgb {
        Rgb::parse("ff00ff").unwrap()
    }

    #[test]
    fn camera_during_call_uses_camera_color() {
        let (mut e, t) = engine(AfterMic::Restore);
        assert_eq!(e.on_input(vec![user("zoom")], true, t), Some(magenta()));
    }

    #[test]
    fn camera_alone_counts_as_active() {
        let (mut e, t) = engine(AfterMic::Restore);
        assert_eq!(e.on_input(vec![], true, t), Some(magenta()));
        assert!(e.overriding());
    }

    #[test]
    fn camera_switching_during_call_changes_color() {
        let (mut e, t) = engine(AfterMic::Restore);
        assert_eq!(e.on_input(vec![user("zoom")], false, t), Some(Rgb::RED));
        assert_eq!(e.on_input(vec![user("zoom")], true, t), Some(magenta()));
        assert_eq!(e.on_input(vec![user("zoom")], true, t), None);
        assert_eq!(e.on_input(vec![user("zoom")], false, t), Some(Rgb::RED));
    }

    #[test]
    fn camera_ignored_when_switched_off() {
        let cfg = Config { camera: false, ..Config::default() };
        let mut e = Engine::new(cfg, None);
        assert_eq!(e.on_input(vec![], true, Instant::now()), None);
        assert!(!e.camera_on());
    }

    #[test]
    fn new_camera_color_applies_immediately() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_input(vec![], true, t);
        let cfg = Config { camera_color: Rgb::GREEN, ..e.config().clone() };
        assert_eq!(e.set_config(cfg), Some(Rgb::GREEN));
    }

    #[test]
    fn color_chosen_during_call_stays_until_call_ends() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        let yellow = Rgb::parse("ffff00").unwrap();
        e.on_user_color(yellow);
        assert_eq!(e.on_mic(vec![user("zoom")], t + Duration::from_secs(1)), None);
        assert_eq!(e.frame(yellow), Frame::solid(yellow));
    }

    // --- Auf Sendung ----------------------------------------------------

    #[test]
    fn live_follows_activity_with_delay() {
        let (mut e, t) = engine(AfterMic::Restore);
        assert!(!e.live());
        e.on_mic(vec![user("zoom")], t);
        assert!(e.live());
        e.on_mic(vec![], t + Duration::from_secs(1));
        assert!(e.live(), "kurze Pause zählt mit");
        e.on_mic(vec![], t + Duration::from_secs(4));
        assert!(!e.live());
    }

    #[test]
    fn live_does_not_depend_on_automatic() {
        let cfg = Config { enabled: false, ..Config::default() };
        let mut e = Engine::new(cfg, None);
        e.on_mic(vec![user("zoom")], Instant::now());
        assert!(e.live());
    }

    #[test]
    fn not_live_while_asleep_but_live_while_locked() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        e.set_away(Away::Locked, true);
        assert!(e.live());
        e.set_away(Away::Sleep, true);
        assert!(!e.live());
    }

    // --- Seiten der Flag --------------------------------------------------

    #[test]
    fn frame_is_solid_by_default() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.frame(Rgb::RED), Frame::solid(Rgb::RED));
    }

    #[test]
    fn mic_color_on_one_side_keeps_resting_color_on_the_other() {
        let cfg = Config { side: Side::A, ..Config::default() };
        let mut e = Engine::new(cfg, Some(blue()));
        let c = e.on_mic(vec![user("zoom")], Instant::now()).unwrap();
        assert_eq!(e.frame(c), Frame { a: Rgb::RED, b: blue() });
        let cfg = Config { side: Side::B, ..e.config().clone() };
        e.set_config(cfg);
        assert_eq!(e.frame(c), Frame { a: blue(), b: Rgb::RED });
    }

    #[test]
    fn resting_color_is_solid_even_with_side_setting() {
        let cfg = Config { side: Side::A, ..Config::default() };
        let e = Engine::new(cfg, Some(blue()));
        assert_eq!(e.frame(blue()), Frame::solid(blue()));
    }

    #[test]
    fn webhook_cannot_split_sides() {
        let cfg = Config {
            side: Side::A,
            transport: Transport::Webhook { luxafor_id: "x".into() },
            ..Config::default()
        };
        let mut e = Engine::new(cfg, Some(blue()));
        let c = e.on_mic(vec![user("zoom")], Instant::now()).unwrap();
        assert_eq!(e.frame(c), Frame::solid(Rgb::RED));
    }

    #[test]
    fn changing_side_during_call_resends() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        let cfg = Config { side: Side::A, ..e.config().clone() };
        assert_eq!(e.set_config(cfg), Some(Rgb::RED));
    }
}
