//! Entscheidungslogik ohne Seiteneffekte: Mikro an → Mikrofarbe,
//! Mikro aus → vorherige oder feste Farbe. Gut testbar, weil Zeit und
//! Mikrofon-Zustand von außen hineingereicht werden.

use std::time::{Duration, Instant};

use crate::color::Rgb;
use crate::lamp::Transport;
use crate::mic::MicUser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterMic {
    /// Zur Farbe zurück, die vor dem Mikrofon galt
    Restore,
    /// Immer auf die „Mikro aus“-Farbe
    Fixed,
}

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
            ignored: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct Engine {
    cfg: Config,
    /// Die „ruhende“ Farbe, zu der nach dem Mikrofon zurückgekehrt wird
    base: Option<Rgb>,
    overriding: bool,
    last_active: Option<Instant>,
    users: Vec<MicUser>,
}

impl Engine {
    pub fn new(cfg: Config, base: Option<Rgb>) -> Self {
        Engine { cfg, base, overriding: false, last_active: None, users: Vec::new() }
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

    fn is_ignored(&self, u: &MicUser) -> bool {
        self.cfg.ignored.iter().any(|i| {
            let i = i.trim();
            !i.is_empty() && (i.eq_ignore_ascii_case(&u.bundle_id) || i.eq_ignore_ascii_case(&u.name))
        })
    }

    fn restore_color(&self) -> Rgb {
        match self.cfg.after_mic {
            AfterMic::Restore => self.base.unwrap_or(self.cfg.mic_off_color),
            AfterMic::Fixed => self.cfg.mic_off_color,
        }
    }

    /// Neuer Mikrofon-Zustand. Liefert eine Farbe, wenn die Lampe umgeschaltet werden soll.
    pub fn on_mic(&mut self, users: Vec<MicUser>, now: Instant) -> Option<Rgb> {
        self.users = users.into_iter().filter(|u| !self.is_ignored(u)).collect();
        let active = !self.users.is_empty();

        if active {
            self.last_active = Some(now);
            if self.cfg.enabled && !self.overriding {
                self.overriding = true;
                return Some(self.cfg.mic_on_color);
            }
            return None;
        }

        if self.overriding {
            let since = self.last_active.map(|t| now.duration_since(t)).unwrap_or(Duration::MAX);
            if since >= self.cfg.release_delay {
                return Some(self.end_override());
            }
        }
        None
    }

    fn end_override(&mut self) -> Rgb {
        self.overriding = false;
        let c = self.restore_color();
        self.base = Some(c);
        c
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
        let old = std::mem::replace(&mut self.cfg, cfg);
        if self.overriding && !self.cfg.enabled {
            return Some(self.end_override());
        }
        if self.overriding && old.mic_on_color != self.cfg.mic_on_color {
            return Some(self.cfg.mic_on_color);
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
}
