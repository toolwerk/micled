//! Swift-Schnittstelle von MicLED. Farben werden als `rrggbb` übergeben,
//! `000000` bedeutet „aus“.

use std::sync::Arc;
use std::time::Duration;

use micled_core as core;
use micled_core::Rgb;

uniffi::setup_scaffolding!();

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    Usb,
    Webhook,
    /// Keine Lampe: Status nur auf Displays, in Slack und am Webhook
    NoLamp,
}

/// Lampe oder Raum auf der Hue Bridge
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HueTargetKind {
    Light,
    Room,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterMic {
    /// Zur Farbe von vor dem Mikrofon zurück
    Restore,
    /// Immer die „Mikro aus“-Farbe
    Fixed,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwayReason {
    /// Ruhezustand des Macs
    Sleep,
    /// Bildschirm ausgeschaltet
    DisplaySleep,
    /// Bildschirm gesperrt
    Locked,
}

/// Befehl aus einer `micled://`-Adresse
#[derive(uniffi::Enum, Clone, Debug, PartialEq, Eq)]
pub enum UrlCommand {
    SetColor { hex: String },
    Off,
    /// `on = None` schaltet um
    Automatic { on: Option<bool> },
    /// Konzentrations-Timer, `minutes = 0` beendet ihn
    Timer { minutes: u32 },
    /// Fokus von macOS an/aus (aus einer Kurzbefehle-Automation)
    Focus { on: bool },
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct MicLedConfig {
    pub enabled: bool,
    pub transport: TransportKind,
    pub luxafor_id: String,
    pub mic_on_color: String,
    pub mic_off_color: String,
    pub after_mic: AfterMic,
    pub release_delay_ms: u32,
    pub ignored: Vec<String>,
    /// Lampe aus bei Ruhezustand, Bildschirm aus und Sperre
    pub off_when_away: bool,
    /// Kamera an zählt als aktiv und hat eine eigene Farbe
    pub camera_enabled: bool,
    pub camera_color: String,
    /// POST bei „Auf Sendung“ an/aus, leer = aus
    pub on_air_url: String,
    pub reminder_color: String,
    pub reminder_lead_s: u32,
    /// Termine direkt aus Nextcloud; ist eines der drei Felder leer, ist es aus
    pub nextcloud_server: String,
    pub nextcloud_user: String,
    /// App-Passwort (liegt in der App im Schlüsselbund)
    pub nextcloud_password: String,
    /// Eigene Farbe je Programm, erster passender Eintrag gewinnt
    pub app_colors: Vec<AppColor>,
    pub timer_color: String,
    /// Fokus von macOS auf der Lampe zeigen
    pub focus_enabled: bool,
    pub focus_color: String,
    /// Abends dimmen: Zeitspanne in Minuten nach Mitternacht, Helligkeit in Prozent
    pub dim_enabled: bool,
    pub dim_from_min: u16,
    pub dim_to_min: u16,
    pub dim_percent: u8,
    /// Slack-Status bei „Auf Sendung“; leerer Token = aus
    pub slack_token: String,
    pub slack_text: String,
    pub slack_emoji: String,
    /// Adresse eines Pixoo 64 im eigenen Netz; leer = aus
    pub pixoo_host: String,
    /// Adresse einer Pixeluhr mit AWTRIX 3 im eigenen Netz; leer = aus
    pub awtrix_host: String,
    /// Hue Bridge; ist Adresse, Schlüssel, Fingerabdruck oder Ziel leer, ist es aus
    pub hue_host: String,
    /// App-Schlüssel vom Koppeln (liegt in der App im Schlüsselbund)
    pub hue_key: String,
    pub hue_fingerprint: String,
    pub hue_target_kind: HueTargetKind,
    pub hue_target_id: String,
    /// Adressen von Govee-Lampen; leere fallen weg, doppelte zählen einmal
    pub govee_hosts: Vec<String>,
    /// Gekoppelte Nanoleaf-Lampen; ohne Adresse oder Schlüssel fallen sie weg
    pub nanoleaf: Vec<NanoleafDevice>,
    /// Lichtstreifen mit Robobloq-Controller per USB
    pub robobloq: bool,
}

/// Nanoleaf-Lampe mit Schlüssel vom Koppeln (liegt in der App im Schlüsselbund)
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct NanoleafDevice {
    pub host: String,
    pub key: String,
}

/// Gerät aus einer Suche im eigenen Netz
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FoundDevice {
    /// Adresse zum Eintragen (bei Nanoleaf mit Port)
    pub host: String,
    /// Modell oder Name des Geräts
    pub name: String,
}

/// Ergebnis von [`govee_discover`] und [`nanoleaf_discover`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct FoundDevices {
    pub items: Vec<FoundDevice>,
    pub error: Option<String>,
}

/// Ergebnis von [`nanoleaf_pair`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct NanoleafPair {
    /// Schlüssel, leer bei Fehler
    pub key: String,
    pub error: Option<String>,
}

/// Ergebnis von [`hue_pair`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct HuePair {
    /// App-Schlüssel, leer bei Fehler
    pub key: String,
    /// SHA-256 des Zertifikats der Bridge, leer bei Fehler
    pub fingerprint: String,
    pub error: Option<String>,
}

/// Lampe oder Raum zur Auswahl
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq, Hash)]
pub struct HueTarget {
    pub kind: HueTargetKind,
    pub id: String,
    pub name: String,
}

/// Ergebnis von [`hue_targets`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct HueTargets {
    /// Erst die Räume, dann die Lampen, jeweils nach Namen
    pub items: Vec<HueTarget>,
    pub error: Option<String>,
}

/// Ergebnis von [`pixoo_check`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct PixooCheck {
    pub error: Option<String>,
}

/// Ergebnis von [`awtrix_check`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct AwtrixCheck {
    pub error: Option<String>,
}

/// Ergebnis von [`robobloq_check`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct RobobloqCheck {
    /// Zahl der LEDs, 0 bei Fehler
    pub leds: u32,
    /// Firmware wie „1.9.4“, leer bei Fehler
    pub firmware: String,
    pub error: Option<String>,
}

/// Ergebnis von [`slack_check`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct SlackCheck {
    /// „Benutzer in Workspace“
    pub name: Option<String>,
    pub error: Option<String>,
}

/// Farbe für ein Programm (Bundle-ID oder Prozessname)
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AppColor {
    pub key: String,
    pub hex: String,
}

/// Ergebnis von [`nextcloud_check`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct NextcloudCheck {
    /// Gefundene Kalender mit Terminen
    pub calendars: u32,
    pub error: Option<String>,
}

/// Summen einer Kalenderwoche (ISO 8601, Beginn Montag 00:00 Ortszeit)
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct WeekStats {
    /// Montag 00:00 Ortszeit (Unix-Sekunden)
    pub start: u64,
    /// Jahr der ISO-Woche
    pub year: i32,
    pub week: u32,
    pub live_seconds: u64,
    /// Calls, die in der Woche begonnen haben
    pub calls: u32,
    pub focus_seconds: u64,
}

impl From<micled_core::stats::WeekSum> for WeekStats {
    fn from(w: micled_core::stats::WeekSum) -> Self {
        WeekStats {
            start: w.start,
            year: w.year,
            week: w.week,
            live_seconds: w.live,
            calls: w.calls,
            focus_seconds: w.focus,
        }
    }
}

#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct WeeklyStats {
    /// Letzte Kalenderwochen, die laufende zuletzt
    pub weeks: Vec<WeekStats>,
    /// Vorwoche bis zum selben Wochentag und zur selben Uhrzeit wie jetzt
    pub last_week_to_date: WeekStats,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct MicUser {
    pub pid: i32,
    pub bundle_id: String,
    pub name: String,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct MicLedStatus {
    pub enabled: bool,
    pub mic_users: Vec<MicUser>,
    /// Kamera in Benutzung
    pub camera: bool,
    /// Mikro- bzw. Kamerafarbe ist gerade aktiv
    pub overriding: bool,
    /// Ende des Konzentrations-Timers (Unix-Sekunden)
    pub timer_until: Option<u64>,
    /// Fokus von macOS ist an
    pub focus: bool,
    /// Auf Sendung (Mikro oder Kamera, kurze Pausen eingeschlossen)
    pub live: bool,
    /// Farbe nach dem Mikrofon, `None` = noch unbekannt
    pub base_color: Option<String>,
    /// Zuletzt gesetzte Farbe, `None` = unbekannt
    pub shown_color: Option<String>,
    pub error: Option<String>,
}

#[uniffi::export(with_foreign)]
pub trait StatusObserver: Send + Sync {
    fn status_changed(&self, status: MicLedStatus);
}

fn color(s: &str, fallback: Rgb) -> Rgb {
    Rgb::parse(s).unwrap_or(fallback)
}

fn hue(host: &str, key: String, fingerprint: String, kind: HueTargetKind, id: String) -> Option<core::hue::Hue> {
    let host = core::pixoo::normalize(host)?;
    let filled = [&key, &fingerprint, &id].iter().all(|s| !s.trim().is_empty());
    filled.then_some(core::hue::Hue {
        host,
        key,
        fingerprint,
        target: core::hue::Target {
            kind: match kind {
                HueTargetKind::Light => core::hue::Kind::Light,
                HueTargetKind::Room => core::hue::Kind::Room,
            },
            id,
        },
    })
}

fn govee(hosts: &[String]) -> Vec<core::govee::Govee> {
    let mut out: Vec<core::govee::Govee> = Vec::new();
    for host in hosts.iter().filter_map(|h| core::govee::normalize(h)) {
        if !out.iter().any(|g| g.host == host) {
            out.push(core::govee::Govee { host });
        }
    }
    out
}

fn nanoleaf(devices: &[NanoleafDevice]) -> Vec<core::nanoleaf::Nanoleaf> {
    let mut out: Vec<core::nanoleaf::Nanoleaf> = Vec::new();
    for d in devices {
        let (Some(host), key) = (core::nanoleaf::normalize(&d.host), d.key.trim()) else { continue };
        if !key.is_empty() && !out.iter().any(|n| n.host == host) {
            out.push(core::nanoleaf::Nanoleaf { host, key: key.to_string() });
        }
    }
    out
}

fn found(result: Result<Vec<(String, String)>, String>) -> FoundDevices {
    match result {
        Ok(list) => FoundDevices { items: list.into_iter().map(|(host, name)| FoundDevice { host, name }).collect(), error: None },
        Err(e) => FoundDevices { items: vec![], error: Some(e) },
    }
}

fn nextcloud(server: String, user: String, password: String) -> Option<core::Nextcloud> {
    let filled = [&server, &user, &password].iter().all(|s| !s.trim().is_empty());
    filled.then_some(core::Nextcloud { server, user, password })
}

impl From<MicLedConfig> for core::Config {
    fn from(c: MicLedConfig) -> Self {
        core::Config {
            nextcloud: nextcloud(c.nextcloud_server, c.nextcloud_user, c.nextcloud_password),
            timer_color: color(&c.timer_color, Rgb { r: 0x80, g: 0, b: 0xff }),
            focus_enabled: c.focus_enabled,
            slack: (!c.slack_token.trim().is_empty() && !c.slack_text.trim().is_empty()).then(|| core::slack::Slack {
                token: c.slack_token,
                text: c.slack_text.trim().to_string(),
                emoji: c.slack_emoji.trim().to_string(),
            }),
            dim: c.dim_enabled.then_some(core::dim::Dim {
                from_min: c.dim_from_min,
                to_min: c.dim_to_min,
                percent: c.dim_percent,
            }),
            focus_color: color(&c.focus_color, Rgb { r: 0xff, g: 0x88, b: 0 }),
            app_colors: c.app_colors.iter().filter_map(|a| Some((a.key.clone(), Rgb::parse(&a.hex)?))).collect(),
            enabled: c.enabled,
            transport: match c.transport {
                TransportKind::Usb => core::Transport::Usb,
                TransportKind::Webhook => core::Transport::Webhook { luxafor_id: c.luxafor_id },
                TransportKind::NoLamp => core::Transport::None,
            },
            pixoo: core::pixoo::normalize(&c.pixoo_host),
            awtrix: core::pixoo::normalize(&c.awtrix_host),
            hue: hue(&c.hue_host, c.hue_key, c.hue_fingerprint, c.hue_target_kind, c.hue_target_id),
            govee: govee(&c.govee_hosts),
            nanoleaf: nanoleaf(&c.nanoleaf),
            robobloq: c.robobloq,
            mic_on_color: color(&c.mic_on_color, Rgb::RED),
            mic_off_color: color(&c.mic_off_color, Rgb::GREEN),
            after_mic: match c.after_mic {
                AfterMic::Restore => core::AfterMic::Restore,
                AfterMic::Fixed => core::AfterMic::Fixed,
            },
            release_delay: Duration::from_millis(c.release_delay_ms as u64),
            ignored: c.ignored,
            off_when_away: c.off_when_away,
            camera: c.camera_enabled,
            camera_color: color(&c.camera_color, Rgb::MAGENTA),
            on_air_url: c.on_air_url,
            reminder_color: color(&c.reminder_color, Rgb::YELLOW),
            reminder_lead: Duration::from_secs(c.reminder_lead_s as u64),
        }
    }
}

impl From<core::MicUser> for MicUser {
    fn from(u: core::MicUser) -> Self {
        MicUser { pid: u.pid, bundle_id: u.bundle_id, name: u.name }
    }
}

impl From<core::Status> for MicLedStatus {
    fn from(s: core::Status) -> Self {
        MicLedStatus {
            enabled: s.enabled,
            mic_users: s.mic_users.into_iter().map(Into::into).collect(),
            camera: s.camera,
            overriding: s.overriding,
            timer_until: s.timer_until,
            focus: s.focus,
            live: s.live,
            base_color: s.base.map(|c| c.hex()),
            shown_color: s.shown.map(|c| c.hex()),
            error: s.error,
        }
    }
}

struct Bridge(Arc<dyn StatusObserver>);

impl core::StatusListener for Bridge {
    fn status_changed(&self, status: core::Status) {
        self.0.status_changed(status.into());
    }
}

/// Läuft im Hintergrund, solange das Objekt lebt.
#[derive(uniffi::Object)]
pub struct MicLedController {
    monitor: core::Monitor,
}

#[uniffi::export]
impl MicLedController {
    #[uniffi::constructor]
    pub fn new(config: MicLedConfig, observer: Arc<dyn StatusObserver>) -> Arc<Self> {
        let monitor = core::Monitor::start(config.into(), None, Arc::new(Bridge(observer)));
        Arc::new(MicLedController { monitor })
    }

    pub fn update_config(&self, config: MicLedConfig) {
        self.monitor.update_config(config.into());
    }

    /// Farbe direkt setzen. Sie gilt sofort und ist danach die Farbe,
    /// zu der nach dem Mikrofon zurückgekehrt wird.
    pub fn set_color(&self, hex: String) {
        if let Some(c) = Rgb::parse(&hex) {
            self.monitor.set_color(c);
        }
    }

    /// Ruhezustand, Bildschirm aus oder Sperre beginnt (`away = true`) bzw. endet
    pub fn set_away(&self, reason: AwayReason, away: bool) {
        let r = match reason {
            AwayReason::Sleep => core::Away::Sleep,
            AwayReason::DisplaySleep => core::Away::DisplaySleep,
            AwayReason::Locked => core::Away::Locked,
        };
        self.monitor.set_away(r, away);
    }

    /// Startzeiten der nächsten Termine (Unix-Sekunden) für die Vorwarnung
    pub fn set_upcoming(&self, starts: Vec<u64>) {
        self.monitor.set_upcoming(starts);
    }

    /// Sekunden auf Sendung seit `since` (Unix-Sekunden), laufender Call eingeschlossen
    pub fn live_seconds_since(&self, since: u64) -> u64 {
        self.monitor.live_seconds_since(since)
    }

    /// Konzentrations-Timer für `minutes` Minuten, 0 beendet ihn
    pub fn start_timer(&self, minutes: u32) {
        self.monitor.start_timer(minutes);
    }

    /// Fokus von macOS beginnt oder endet
    pub fn set_focus(&self, on: bool) {
        self.monitor.set_focus(on);
    }

    /// „Testen“ beim Lichtstreifen: Geräteinfo lesen, zwei Sekunden rot, danach
    /// wieder der Zustand von MicLED. Blockiert bis zu einigen Sekunden, nicht
    /// im Haupt-Thread aufrufen.
    pub fn test_strip(&self) -> RobobloqCheck {
        self.monitor.test_strip().into()
    }

    /// Sekunden auf Sendung je Tag; `bounds` sind aufsteigende lokale Mitternachten
    pub fn live_seconds_per_day(&self, bounds: Vec<u64>) -> Vec<u64> {
        self.monitor.live_seconds_per_day(&bounds)
    }

    /// Gespeicherte Calls als CSV (`start,end,seconds`, UTC)
    pub fn calls_csv(&self) -> String {
        self.monitor.calls_csv()
    }

    /// Summen der letzten `weeks` Kalenderwochen und die Vorwoche bis jetzt
    pub fn weekly_stats(&self, weeks: u32) -> WeeklyStats {
        let (list, last) = self.monitor.weekly(weeks as usize);
        WeeklyStats { weeks: list.into_iter().map(Into::into).collect(), last_week_to_date: last.into() }
    }

    /// Summen je Kalenderwoche als CSV (`week,start,live_seconds,calls,focus_seconds`)
    pub fn weeks_csv(&self) -> String {
        self.monitor.weeks_csv()
    }

    pub fn turn_off(&self) {
        self.monitor.set_color(Rgb::OFF);
    }

    pub fn status(&self) -> MicLedStatus {
        self.monitor.status().into()
    }

    pub fn stop(&self) {
        self.monitor.stop();
    }
}

/// Programme, die ab Werk ignoriert werden (Vorgabe für die Einstellungen)
#[uniffi::export]
pub fn default_ignored() -> Vec<String> {
    core::DEFAULT_IGNORED.iter().map(|s| s.to_string()).collect()
}

/// `micled://`-Adresse auswerten, `None` bei unbekanntem Befehl
#[uniffi::export]
pub fn parse_url_command(url: String) -> Option<UrlCommand> {
    core::command::parse(&url).map(|c| match c {
        core::command::Command::SetColor(c) => UrlCommand::SetColor { hex: c.hex() },
        core::command::Command::Off => UrlCommand::Off,
        core::command::Command::Automatic(on) => UrlCommand::Automatic { on },
        core::command::Command::Timer(minutes) => UrlCommand::Timer { minutes },
        core::command::Command::Focus(on) => UrlCommand::Focus { on },
    })
}

/// Farbe aus Text (`ff8800`, `#f80`, `rot`) als `rrggbb`, `None` wenn unlesbar
#[uniffi::export]
pub fn parse_color(text: String) -> Option<String> {
    Rgb::parse(&text).map(|c| c.hex())
}

/// Zugang zu Nextcloud prüfen (blockiert bis zu einigen Sekunden, nicht im
/// Haupt-Thread aufrufen)
#[uniffi::export]
pub fn nextcloud_check(server: String, user: String, password: String) -> NextcloudCheck {
    let result = match nextcloud(server, user, password) {
        Some(nc) => core::caldav::check(&nc),
        None => Err("Server, Benutzer und App-Passwort eintragen".into()),
    };
    match result {
        Ok(n) => NextcloudCheck { calendars: n as u32, error: None },
        Err(e) => NextcloudCheck { calendars: 0, error: Some(e) },
    }
}

/// Slack-Token prüfen (blockiert, nicht im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn slack_check(token: String) -> SlackCheck {
    match core::slack::check(&token) {
        Ok(name) => SlackCheck { name: Some(name), error: None },
        Err(e) => SlackCheck { name: None, error: Some(e) },
    }
}

/// Pixoo 64 unter `host` erreichbar? (blockiert bis zu drei Sekunden, nicht
/// im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn pixoo_check(host: String) -> PixooCheck {
    let result = match core::pixoo::normalize(&host) {
        Some(h) => core::pixoo::check(&h),
        None => Err("Adresse des Pixoo eintragen".into()),
    };
    PixooCheck { error: result.err() }
}

/// AWTRIX-Uhr unter `host` erreichbar? (blockiert bis zu drei Sekunden, nicht
/// im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn awtrix_check(host: String) -> AwtrixCheck {
    let result = match core::pixoo::normalize(&host) {
        Some(h) => core::awtrix::check(&h),
        None => Err("Adresse der Uhr eintragen".into()),
    };
    AwtrixCheck { error: result.err() }
}

/// Mit der Hue Bridge unter `host` koppeln; vorher den Knopf an der Bridge
/// drücken (blockiert bis zu einigen Sekunden, nicht im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn hue_pair(host: String) -> HuePair {
    let result = match core::pixoo::normalize(&host) {
        Some(h) => core::hue::pair(&h),
        None => Err("Adresse der Bridge eintragen".into()),
    };
    match result {
        Ok((key, fingerprint)) => HuePair { key, fingerprint, error: None },
        Err(e) => HuePair { key: String::new(), fingerprint: String::new(), error: Some(e) },
    }
}

/// Räume und Lampen der gekoppelten Bridge (blockiert, nicht im Haupt-Thread
/// aufrufen)
#[uniffi::export]
pub fn hue_targets(host: String, key: String, fingerprint: String) -> HueTargets {
    // Das Ziel zählt hier nicht, nur der Zugang
    let result = match hue(&host, key, fingerprint, HueTargetKind::Light, "-".into()) {
        Some(h) => core::hue::targets(&h),
        None => Err("Erst mit der Bridge koppeln".into()),
    };
    match result {
        Ok(list) => HueTargets {
            items: list
                .into_iter()
                .map(|(t, name)| HueTarget {
                    kind: match t.kind {
                        core::hue::Kind::Light => HueTargetKind::Light,
                        core::hue::Kind::Room => HueTargetKind::Room,
                    },
                    id: t.id,
                    name,
                })
                .collect(),
            error: None,
        },
        Err(e) => HueTargets { items: vec![], error: Some(e) },
    }
}

/// Govee-Lampen mit „LAN Control“ im eigenen Netz suchen (blockiert etwa zwei
/// Sekunden, nicht im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn govee_discover() -> FoundDevices {
    found(core::govee::discover())
}

/// Nanoleaf-Lampen per Bonjour suchen (blockiert etwa drei Sekunden, nicht im
/// Haupt-Thread aufrufen)
#[uniffi::export]
pub fn nanoleaf_discover() -> FoundDevices {
    found(core::nanoleaf::discover())
}

/// Mit der Nanoleaf-Lampe unter `host` koppeln; vorher den Einschaltknopf
/// 5–7 Sekunden halten (blockiert bis zu drei Sekunden, nicht im Haupt-Thread
/// aufrufen)
#[uniffi::export]
pub fn nanoleaf_pair(host: String) -> NanoleafPair {
    let result = match core::nanoleaf::normalize(&host) {
        Some(h) => core::nanoleaf::pair(&h),
        None => Err("Adresse der Nanoleaf-Lampe eintragen".into()),
    };
    match result {
        Ok(key) => NanoleafPair { key, error: None },
        Err(e) => NanoleafPair { key: String::new(), error: Some(e) },
    }
}

/// Geräteinfo des Lichtstreifens per USB lesen (blockiert bis zu einigen
/// Sekunden, nicht im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn robobloq_check() -> RobobloqCheck {
    core::robobloq::check().into()
}

impl From<Result<core::robobloq::Info, String>> for RobobloqCheck {
    fn from(r: Result<core::robobloq::Info, String>) -> Self {
        match r {
            Ok(info) => RobobloqCheck { leds: info.leds as u32, firmware: info.firmware_text(), error: None },
            Err(e) => RobobloqCheck { leds: 0, firmware: String::new(), error: Some(e) },
        }
    }
}

/// Aktuelle Mikrofon-Nutzer (für die Ignorieren-Liste in den Einstellungen)
#[uniffi::export]
pub fn current_mic_users() -> Vec<MicUser> {
    core::mic::users().unwrap_or_default().into_iter().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn govee_hosts_are_cleaned_and_deduplicated() {
        let hosts = ["192.168.1.31", " ", "http://192.168.1.31/", "192.168.1.32"].map(String::from);
        let list: Vec<String> = govee(&hosts).into_iter().map(|g| g.host).collect();
        assert_eq!(list, ["192.168.1.31", "192.168.1.32"]);
    }

    #[test]
    fn nanoleaf_without_address_or_key_is_dropped() {
        let d = |host: &str, key: &str| NanoleafDevice { host: host.into(), key: key.into() };
        let list = nanoleaf(&[d("10.0.0.4", "k1"), d("10.0.0.4:16021", "k2"), d("10.0.0.5", " "), d("", "k3")]);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].host.as_str(), list[0].key.as_str()), ("10.0.0.4:16021", "k1"));
    }
}
