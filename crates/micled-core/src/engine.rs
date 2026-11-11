//! Entscheidungslogik ohne Seiteneffekte: Mikro oder Kamera an → Mikro- bzw.
//! Kamerafarbe, danach vorherige oder feste Farbe. Gut testbar, weil Zeit,
//! Mikrofon- und Kamerazustand von außen hineingereicht werden.

use std::time::{Duration, Instant};

use crate::caldav::Nextcloud;
use crate::color::Rgb;
use crate::dim::Dim;
use crate::display::Scene;
use crate::slack::Slack;
use crate::lamp::Transport;
use crate::mic::MicUser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterMic {
    /// Zur Farbe zurück, die vor dem Mikrofon galt
    Restore,
    /// Immer auf die „Mikro aus“-Farbe
    Fixed,
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
    /// Ziel für „Auf Sendung“-Meldungen (leer = aus), siehe [`crate::onair`]
    pub on_air_url: String,
    /// Farbe der Vorwarnung vor Terminen
    pub reminder_color: Rgb,
    /// So lange vor Terminbeginn wird gewarnt
    pub reminder_lead: Duration,
    /// Termine zusätzlich direkt aus Nextcloud lesen (`None` = aus)
    pub nextcloud: Option<Nextcloud>,
    /// Eigene Farbe je Programm (Bundle-ID oder Prozessname). Bei mehreren
    /// aktiven Programmen gewinnt der erste passende Eintrag.
    pub app_colors: Vec<(String, Rgb)>,
    /// Farbe während eines Konzentrations-Timers
    pub timer_color: Rgb,
    /// Fokus von macOS auf der Lampe zeigen
    pub focus_enabled: bool,
    pub focus_color: Rgb,
    /// Abends dimmen (`None` = aus)
    pub dim: Option<Dim>,
    /// Slack-Status bei „Auf Sendung“ (`None` = aus)
    pub slack: Option<Slack>,
    /// Adresse eines Pixoo 64 im eigenen Netz (`None` = aus)
    pub pixoo: Option<String>,
    /// Adresse einer Pixeluhr mit AWTRIX 3 im eigenen Netz (`None` = aus)
    pub awtrix: Option<String>,
    /// Hue-Lampe oder -Raum über die Hue Bridge (`None` = aus)
    pub hue: Option<crate::hue::Hue>,
    /// Govee-Lampen im eigenen Netz (leer = aus)
    pub govee: Vec<crate::govee::Govee>,
    /// Gekoppelte Nanoleaf-Lampen (leer = aus)
    pub nanoleaf: Vec<crate::nanoleaf::Nanoleaf>,
    /// WLED-Geräte im eigenen Netz (leer = aus)
    pub wled: Vec<crate::wled::Wled>,
    /// Lichtstreifen mit Robobloq-Controller per USB
    pub robobloq: bool,
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
            on_air_url: String::new(),
            reminder_color: Rgb::YELLOW,
            reminder_lead: Duration::from_secs(120),
            nextcloud: None,
            app_colors: Vec::new(),
            timer_color: Rgb { r: 0x80, g: 0x00, b: 0xff },
            focus_enabled: false,
            focus_color: Rgb { r: 0xff, g: 0x88, b: 0x00 },
            dim: None,
            slack: None,
            pixoo: None,
            awtrix: None,
            hue: None,
            govee: Vec::new(),
            nanoleaf: Vec::new(),
            wled: Vec::new(),
            robobloq: false,
        }
    }
}

/// Passt ein Eintrag (Bundle-ID oder Prozessname) auf das Programm?
fn matches(key: &str, u: &MicUser) -> bool {
    let k = key.trim();
    !k.is_empty() && (k.eq_ignore_ascii_case(&u.bundle_id) || k.eq_ignore_ascii_case(&u.name))
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
    /// Kamera und Farbe des Calls bei der letzten Aktivität; gilt auch in der Wartezeit
    live_look: (bool, Rgb),
    /// Gesetzte `Away`-Bits
    away: u8,
    /// Ende des Konzentrations-Timers (Unix-Sekunden)
    timer: Option<u64>,
    /// Fokus von macOS ist an
    focus: bool,
    /// Während des Fokus wurde eine Farbe gewählt; sie gilt bis zum nächsten Wechsel
    focus_suspended: bool,
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
            live_look: (false, Rgb::RED),
            away: 0,
            timer: None,
            focus: false,
            focus_suspended: false,
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
        self.cfg.ignored.iter().any(|i| matches(i, u))
    }

    /// Lampe soll wegen Abwesenheit aus sein
    pub fn dark(&self) -> bool {
        self.cfg.off_when_away && self.away != 0
    }

    /// Farbe, wenn weder Mikrofon noch Kamera aktiv sind. Über der ruhenden
    /// Farbe liegen Timer und Fokus; beide kommen nie in die Statusdatei.
    pub fn resting(&self) -> Rgb {
        if self.dark() {
            Rgb::OFF
        } else if self.timer.is_some() {
            self.cfg.timer_color
        } else if self.focus_shown() {
            self.cfg.focus_color
        } else {
            self.base.unwrap_or(self.cfg.mic_off_color)
        }
    }

    /// Bild für Displays zur Zeit `now` (Unix-Sekunden). Ein Call gilt auch bei
    /// ausgeschalteter Automatik (wie in Slack), sonst wie bei [`Engine::resting`].
    pub fn scene(&self, now: u64) -> Scene {
        if self.live() {
            let (camera, color) = self.live_look;
            Scene::Live { camera, color }
        } else if self.dark() {
            Scene::Idle
        } else if let Some(until) = self.timer {
            Scene::Timer { minutes: until.saturating_sub(now).div_ceil(60).max(1) as u32, color: self.cfg.timer_color }
        } else if self.focus_shown() {
            Scene::Focus { color: self.cfg.focus_color }
        } else {
            Scene::Idle
        }
    }

    fn focus_shown(&self) -> bool {
        self.focus && self.cfg.focus_enabled && !self.focus_suspended
    }

    /// Ende des laufenden Konzentrations-Timers (Unix-Sekunden)
    pub fn timer(&self) -> Option<u64> {
        self.timer
    }

    /// Fokus von macOS ist an (unabhängig davon, ob er angezeigt wird)
    pub fn focus(&self) -> bool {
        self.focus
    }

    /// Farbe, falls sich die ruhende Anzeige geändert hat und kein Call läuft
    fn resting_if_changed(&self, before: Rgb) -> Option<Rgb> {
        let now = self.resting();
        (!self.overriding && now != before).then_some(now)
    }

    /// Konzentrations-Timer bis `until` (Unix-Sekunden) starten oder verlängern
    pub fn start_timer(&mut self, until: u64) -> Option<Rgb> {
        let before = self.resting();
        self.timer = Some(until);
        self.resting_if_changed(before)
    }

    /// Timer beenden (abgelaufen oder abgebrochen)
    pub fn stop_timer(&mut self) -> Option<Rgb> {
        let before = self.resting();
        self.timer = None;
        self.resting_if_changed(before)
    }

    /// Fokus von macOS beginnt oder endet
    pub fn set_focus(&mut self, on: bool) -> Option<Rgb> {
        if self.focus == on {
            return None;
        }
        let before = self.resting();
        self.focus = on;
        self.focus_suspended = false;
        self.resting_if_changed(before)
    }

    /// Kamera vor Programmfarbe vor Mikrofarbe
    fn override_color(&self) -> Rgb {
        if self.camera {
            return self.cfg.camera_color;
        }
        self.cfg
            .app_colors
            .iter()
            .find(|(key, _)| self.users.iter().any(|u| matches(key, u)))
            .map(|&(_, c)| c)
            .unwrap_or(self.cfg.mic_on_color)
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
        // Während eines Calls bleibt die Mikrofarbe
        (!self.overriding).then(|| self.resting())
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
            self.live_look = (self.camera, self.override_color());
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
    /// Beendet einen laufenden Timer und blendet den Fokus bis zu seinem nächsten Wechsel aus.
    pub fn on_user_color(&mut self, c: Rgb) -> Rgb {
        self.on_shared_color(c);
        c
    }

    /// Ein anderes Programm (Stream Deck) hat die Lampe gesetzt.
    pub fn on_shared_color(&mut self, c: Rgb) {
        self.base = Some(c);
        self.timer = None;
        self.focus_suspended = self.focus;
    }

    /// Einstellungen ändern. Liefert eine Farbe, falls das sofort sichtbar werden muss.
    pub fn set_config(&mut self, cfg: Config) -> Option<Rgb> {
        let before = self.resting();
        self.cfg = cfg;
        if !self.cfg.camera {
            self.camera = false;
        }
        if !self.overriding {
            return self.resting_if_changed(before);
        }
        if !self.cfg.enabled {
            return Some(self.end_override());
        }
        if self.override_shown != Some(self.override_color()) {
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

    // --- Farbe je Programm ------------------------------------------------

    fn orange() -> Rgb {
        Rgb::parse("ff8800").unwrap()
    }

    fn with_app_colors() -> (Engine, Instant) {
        let cfg = Config {
            app_colors: vec![("com.example.plaud".into(), orange()), ("zoom".into(), Rgb::YELLOW)],
            ..Config::default()
        };
        (Engine::new(cfg, Some(blue())), Instant::now())
    }

    #[test]
    fn app_color_replaces_mic_color() {
        let (mut e, t) = with_app_colors();
        assert_eq!(e.on_mic(vec![user("plaud")], t), Some(orange()));
    }

    #[test]
    fn app_without_entry_uses_mic_color() {
        let (mut e, t) = with_app_colors();
        assert_eq!(e.on_mic(vec![user("teams")], t), Some(Rgb::RED));
    }

    #[test]
    fn first_entry_wins_with_several_apps() {
        let (mut e, t) = with_app_colors();
        assert_eq!(e.on_mic(vec![user("zoom"), user("plaud")], t), Some(orange()));
    }

    #[test]
    fn camera_beats_app_color() {
        let (mut e, t) = with_app_colors();
        assert_eq!(e.on_input(vec![user("plaud")], true, t), Some(Rgb::MAGENTA));
    }

    #[test]
    fn color_follows_apps_joining_and_leaving() {
        let (mut e, t) = with_app_colors();
        assert_eq!(e.on_mic(vec![user("zoom")], t), Some(Rgb::YELLOW));
        assert_eq!(e.on_mic(vec![user("zoom"), user("plaud")], t), Some(orange()));
        assert_eq!(e.on_mic(vec![user("zoom")], t), Some(Rgb::YELLOW));
        assert_eq!(e.on_mic(vec![user("zoom")], t), None);
    }

    #[test]
    fn changing_app_color_during_call_applies() {
        let (mut e, t) = with_app_colors();
        e.on_mic(vec![user("plaud")], t);
        let cfg = Config { app_colors: vec![("com.example.plaud".into(), Rgb::GREEN)], ..e.config().clone() };
        assert_eq!(e.set_config(cfg), Some(Rgb::GREEN));
    }

    // --- Timer und Fokus --------------------------------------------------

    const VIOLET: Rgb = Rgb { r: 0x80, g: 0x00, b: 0xff };
    const ORANGE: Rgb = Rgb { r: 0xff, g: 0x88, b: 0x00 };

    fn focus_engine() -> (Engine, Instant) {
        let cfg = Config { focus_enabled: true, ..Config::default() };
        (Engine::new(cfg, Some(blue())), Instant::now())
    }

    #[test]
    fn timer_shows_color_and_restores() {
        let (mut e, _) = engine(AfterMic::Restore);
        assert_eq!(e.start_timer(1000), Some(VIOLET));
        assert_eq!(e.timer(), Some(1000));
        assert_eq!(e.start_timer(2000), None, "verlängern ändert nichts Sichtbares");
        assert_eq!(e.stop_timer(), Some(blue()));
        assert_eq!(e.stop_timer(), None);
    }

    #[test]
    fn call_interrupts_timer_and_timer_returns_after() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.start_timer(1000);
        assert_eq!(e.on_mic(vec![user("zoom")], t), Some(Rgb::RED));
        e.on_mic(vec![], t + Duration::from_secs(1));
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(5)), Some(VIOLET));
    }

    #[test]
    fn timer_ending_during_call_shows_nothing_until_call_ends() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.start_timer(1000);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.stop_timer(), None);
        e.on_mic(vec![], t + Duration::from_secs(1));
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(5)), Some(blue()));
    }

    #[test]
    fn choosing_a_color_ends_the_timer() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.start_timer(1000);
        e.on_user_color(Rgb::GREEN);
        assert_eq!(e.timer(), None);
        assert_eq!(e.resting(), Rgb::GREEN);
    }

    #[test]
    fn timer_is_dark_while_locked() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.start_timer(1000);
        assert_eq!(e.set_away(Away::Locked, true), Some(Rgb::OFF));
        assert_eq!(e.set_away(Away::Locked, false), Some(VIOLET));
    }

    #[test]
    fn focus_shows_color_and_restores() {
        let (mut e, _) = focus_engine();
        assert_eq!(e.set_focus(true), Some(ORANGE));
        assert_eq!(e.set_focus(true), None);
        assert_eq!(e.set_focus(false), Some(blue()));
    }

    #[test]
    fn focus_ignored_when_disabled() {
        let (mut e, _) = engine(AfterMic::Restore);
        assert_eq!(e.set_focus(true), None);
        assert!(e.focus());
        let cfg = Config { focus_enabled: true, ..e.config().clone() };
        assert_eq!(e.set_config(cfg), Some(ORANGE), "Einschalten wirkt sofort");
    }

    #[test]
    fn call_beats_focus_and_focus_returns_after() {
        let (mut e, t) = focus_engine();
        e.set_focus(true);
        assert_eq!(e.on_mic(vec![user("zoom")], t), Some(Rgb::RED));
        assert_eq!(e.set_focus(false), None, "während des Calls nichts");
        e.set_focus(true);
        e.on_mic(vec![], t + Duration::from_secs(1));
        assert_eq!(e.on_mic(vec![], t + Duration::from_secs(5)), Some(ORANGE));
    }

    #[test]
    fn timer_beats_focus() {
        let (mut e, _) = focus_engine();
        e.set_focus(true);
        assert_eq!(e.start_timer(1000), Some(VIOLET));
        assert_eq!(e.stop_timer(), Some(ORANGE));
    }

    #[test]
    fn color_chosen_during_focus_holds_until_focus_changes() {
        let (mut e, _) = focus_engine();
        e.set_focus(true);
        e.on_user_color(Rgb::GREEN);
        assert_eq!(e.resting(), Rgb::GREEN);
        assert_eq!(e.set_focus(false), None, "bleibt grün");
        assert_eq!(e.set_focus(true), Some(ORANGE));
    }

    #[test]
    fn focus_and_lock_together() {
        let (mut e, _) = focus_engine();
        e.set_focus(true);
        assert_eq!(e.set_away(Away::Locked, true), Some(Rgb::OFF));
        assert_eq!(e.set_away(Away::Locked, false), Some(ORANGE));
    }

    // --- Bild für Displays ------------------------------------------------

    const NOW: u64 = 1_000_000;

    #[test]
    fn scene_idle_when_nothing_happens() {
        let (e, _) = engine(AfterMic::Restore);
        assert_eq!(e.scene(NOW), Scene::Idle);
    }

    #[test]
    fn scene_live_with_mic_and_camera() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.scene(NOW), Scene::Live { camera: false, color: Rgb::RED });
        e.on_input(vec![user("zoom")], true, t);
        assert_eq!(e.scene(NOW), Scene::Live { camera: true, color: Rgb::MAGENTA });
    }

    #[test]
    fn scene_keeps_look_during_release_delay() {
        let (mut e, t) = with_app_colors();
        e.on_input(vec![user("zoom")], true, t);
        e.on_input(vec![], false, t + Duration::from_secs(1));
        assert!(e.live());
        assert_eq!(e.scene(NOW), Scene::Live { camera: true, color: Rgb::MAGENTA }, "Kamera bleibt bis zum Ende");
        e.on_mic(vec![user("plaud")], t + Duration::from_secs(2));
        e.on_mic(vec![], t + Duration::from_secs(3));
        assert_eq!(e.scene(NOW), Scene::Live { camera: false, color: orange() }, "Programmfarbe bleibt");
        e.on_mic(vec![], t + Duration::from_secs(10));
        assert_eq!(e.scene(NOW), Scene::Idle);
    }

    #[test]
    fn scene_live_uses_app_color() {
        let (mut e, t) = with_app_colors();
        e.on_mic(vec![user("plaud")], t);
        assert_eq!(e.scene(NOW), Scene::Live { camera: false, color: orange() });
    }

    #[test]
    fn scene_live_even_without_automatic() {
        let cfg = Config { enabled: false, ..Config::default() };
        let mut e = Engine::new(cfg, None);
        e.on_mic(vec![user("zoom")], Instant::now());
        assert_eq!(e.scene(NOW), Scene::Live { camera: false, color: Rgb::RED });
    }

    #[test]
    fn scene_timer_counts_whole_minutes() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.start_timer(NOW + 25 * 60);
        assert_eq!(e.scene(NOW), Scene::Timer { minutes: 25, color: VIOLET });
        assert_eq!(e.scene(NOW + 1), Scene::Timer { minutes: 25, color: VIOLET }, "angefangene Minute zählt");
        assert_eq!(e.scene(NOW + 60), Scene::Timer { minutes: 24, color: VIOLET });
        assert_eq!(e.scene(NOW + 25 * 60), Scene::Timer { minutes: 1, color: VIOLET }, "nie 0");
    }

    #[test]
    fn scene_focus() {
        let (mut e, _) = focus_engine();
        e.set_focus(true);
        assert_eq!(e.scene(NOW), Scene::Focus { color: ORANGE });
    }

    #[test]
    fn scene_timer_before_focus() {
        let (mut e, _) = focus_engine();
        e.set_focus(true);
        e.start_timer(NOW + 600);
        assert_eq!(e.scene(NOW), Scene::Timer { minutes: 10, color: VIOLET });
    }

    #[test]
    fn scene_call_before_timer() {
        let (mut e, t) = engine(AfterMic::Restore);
        e.start_timer(NOW + 600);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.scene(NOW), Scene::Live { camera: false, color: Rgb::RED });
    }

    #[test]
    fn scene_idle_while_locked_without_call() {
        let (mut e, t) = focus_engine();
        e.set_focus(true);
        e.start_timer(NOW + 600);
        e.set_away(Away::Locked, true);
        assert_eq!(e.scene(NOW), Scene::Idle);
        e.on_mic(vec![user("zoom")], t);
        assert_eq!(e.scene(NOW), Scene::Live { camera: false, color: Rgb::RED }, "Call auch gesperrt");
    }

    #[test]
    fn scene_idle_when_focus_not_shown() {
        let (mut e, _) = engine(AfterMic::Restore);
        e.set_focus(true);
        assert_eq!(e.scene(NOW), Scene::Idle);
    }
}
