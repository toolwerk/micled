//! Swift-Schnittstelle von MicLED. Farben werden als `rrggbb` übergeben,
//! `000000` bedeutet „aus“.

use std::sync::Arc;
use std::time::Duration;

use micled_core as core;
use micled_core::{tr, Rgb};

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
    /// Vor Terminen blinken
    pub reminders_enabled: bool,
    pub reminder_color: String,
    pub reminder_lead_s: u32,
    /// Zugang zu Nextcloud; ist eines der drei Felder leer, ist er aus
    pub nextcloud_server: String,
    pub nextcloud_user: String,
    /// App-Passwort (liegt in der App im Schlüsselbund)
    pub nextcloud_password: String,
    /// Termine direkt aus Nextcloud lesen
    pub nextcloud_calendar: bool,
    /// Nextcloud-Status bei „Auf Sendung“; leerer Text = aus
    pub nextcloud_status: bool,
    pub nextcloud_status_text: String,
    pub nextcloud_status_emoji: String,
    /// Dazu „Nicht stören“ setzen
    pub nextcloud_status_dnd: bool,
    /// Während eines Calls das Ende des laufenden Termins auf Pixoo und AWTRIX zeigen
    pub meeting_end_enabled: bool,
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
    pub pixoo_states: OutputStates,
    /// Adresse einer Pixeluhr mit AWTRIX 3 im eigenen Netz; leer = aus
    pub awtrix_host: String,
    pub awtrix_states: OutputStates,
    /// Hue Bridge; ist Adresse, Schlüssel, Fingerabdruck oder Ziel leer, ist es aus
    pub hue_host: String,
    /// App-Schlüssel vom Koppeln (liegt in der App im Schlüsselbund)
    pub hue_key: String,
    pub hue_fingerprint: String,
    pub hue_target_kind: HueTargetKind,
    pub hue_target_id: String,
    pub hue_states: OutputStates,
    /// Adressen von Govee-Lampen; leere fallen weg, doppelte zählen einmal
    pub govee_hosts: Vec<String>,
    pub govee_states: OutputStates,
    /// Gekoppelte Nanoleaf-Lampen; ohne Adresse oder Schlüssel fallen sie weg
    pub nanoleaf: Vec<NanoleafDevice>,
    pub nanoleaf_states: OutputStates,
    /// Adressen von WLED-Geräten; leere fallen weg, doppelte zählen einmal
    pub wled_hosts: Vec<String>,
    pub wled_states: OutputStates,
    /// Lichtstreifen mit Robobloq-Controller per USB
    pub robobloq: bool,
    pub robobloq_states: OutputStates,
    /// MQTT-Broker für Home Assistant; leere Adresse oder Geräte-ID = aus
    pub mqtt_host: String,
    /// 0 = Standard (1883, mit TLS 8883)
    pub mqtt_port: u16,
    pub mqtt_tls: bool,
    pub mqtt_user: String,
    /// Liegt in der App im Schlüsselbund
    pub mqtt_password: String,
    /// Feste ID dieses Macs in den Topics
    pub mqtt_device_id: String,
    /// Name des Macs, in Home Assistant „MicLED (<Name>)“
    pub mqtt_device_name: String,
}

/// Zustände, die ein Zusatzgerät zeigt; ein abgewählter gilt dort als frei
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputStates {
    pub call: bool,
    pub timer: bool,
    pub focus: bool,
}

impl From<OutputStates> for core::OutputStates {
    fn from(s: OutputStates) -> Self {
        core::OutputStates { call: s.call, timer: s.timer, focus: s.focus }
    }
}

/// Termin aus den Kalendern von macOS, ohne Titel
#[derive(uniffi::Record, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarEvent {
    /// Beginn in Unix-Sekunden
    pub start: u64,
    /// Ende in Unix-Sekunden, 0 = unbekannt
    pub end: u64,
    pub all_day: bool,
    /// Abgelehnt oder abgesagt
    pub declined: bool,
}

impl From<CalendarEvent> for core::reminder::Event {
    fn from(e: CalendarEvent) -> Self {
        core::reminder::Event { start: e.start, end: (e.end > 0).then_some(e.end), all_day: e.all_day, declined: e.declined }
    }
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
    /// Adresse zum Eintragen (bei Nanoleaf mit Port, bei WLED nur abseits von Port 80)
    pub host: String,
    /// Modell oder Name des Geräts
    pub name: String,
}

/// Ergebnis von [`govee_discover`], [`nanoleaf_discover`] und [`wled_discover`]
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

/// Ergebnis von [`mqtt_check`]
#[derive(uniffi::Record, Clone, Debug)]
pub struct MqttCheck {
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

fn wled(hosts: &[String]) -> Vec<core::wled::Wled> {
    let mut out: Vec<core::wled::Wled> = Vec::new();
    for host in hosts.iter().filter_map(|h| core::wled::normalize(h)) {
        if !out.iter().any(|w| w.host == host) {
            out.push(core::wled::Wled { host });
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

/// Zugang zum Broker; `None` ohne Adresse. Ein Port im Feld der Adresse
/// gilt vor dem Feld „Port“.
fn mqtt_access(host: &str, port: u16, tls: bool, user: &str, password: String) -> Option<core::mqtt::Mqtt> {
    let (host, in_host) = core::mqtt::address(host)?;
    let port = match (in_host.unwrap_or(port), tls) {
        (0, false) => core::mqtt::PORT,
        (0, true) => core::mqtt::TLS_PORT,
        (p, _) => p,
    };
    Some(core::mqtt::Mqtt {
        host,
        port,
        tls,
        user: user.trim().to_string(),
        password,
        device_id: String::new(),
        device_name: String::new(),
    })
}

/// MQTT aus den Einstellungen; ohne Adresse oder Geräte-ID aus
fn mqtt(c: &MicLedConfig) -> Option<core::mqtt::Mqtt> {
    let access = mqtt_access(&c.mqtt_host, c.mqtt_port, c.mqtt_tls, &c.mqtt_user, c.mqtt_password.clone())?;
    let name = c.mqtt_device_name.trim();
    Some(core::mqtt::Mqtt {
        device_id: core::mqtt::clean_id(&c.mqtt_device_id)?,
        device_name: if name.is_empty() { "Mac".into() } else { name.to_string() },
        ..access
    })
}

fn nextcloud(server: String, user: String, password: String) -> Option<core::Nextcloud> {
    let filled = [&server, &user, &password].iter().all(|s| !s.trim().is_empty());
    filled.then_some(core::Nextcloud { server, user, password })
}

/// Status nur mit vollem Zugang, eingeschaltetem Schalter und Text
fn nextcloud_status(nc: Option<core::Nextcloud>, on: bool, text: &str, emoji: &str, dnd: bool) -> Option<core::nc_status::NcStatus> {
    let text = text.trim();
    let nc = nc.filter(|_| on && !text.is_empty())?;
    Some(core::nc_status::NcStatus { nc, text: text.to_string(), emoji: emoji.trim().to_string(), dnd })
}

impl From<MicLedConfig> for core::Config {
    fn from(c: MicLedConfig) -> Self {
        let mqtt = mqtt(&c);
        let nc = nextcloud(c.nextcloud_server, c.nextcloud_user, c.nextcloud_password);
        core::Config {
            mqtt,
            nextcloud_status: nextcloud_status(
                nc.clone(),
                c.nextcloud_status,
                &c.nextcloud_status_text,
                &c.nextcloud_status_emoji,
                c.nextcloud_status_dnd,
            ),
            nextcloud: nc.filter(|_| c.nextcloud_calendar),
            meeting_end: c.meeting_end_enabled,
            reminders: c.reminders_enabled,
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
            pixoo_states: c.pixoo_states.into(),
            awtrix: core::pixoo::normalize(&c.awtrix_host),
            awtrix_states: c.awtrix_states.into(),
            hue: hue(&c.hue_host, c.hue_key, c.hue_fingerprint, c.hue_target_kind, c.hue_target_id),
            hue_states: c.hue_states.into(),
            govee: govee(&c.govee_hosts),
            govee_states: c.govee_states.into(),
            nanoleaf: nanoleaf(&c.nanoleaf),
            nanoleaf_states: c.nanoleaf_states.into(),
            wled: wled(&c.wled_hosts),
            wled_states: c.wled_states.into(),
            robobloq: c.robobloq,
            robobloq_states: c.robobloq_states.into(),
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

    /// Laufende und kommende Termine aus den Kalendern von macOS, für
    /// Vorwarnung und Endzeit
    pub fn set_events(&self, events: Vec<CalendarEvent>) {
        self.monitor.set_events(events.into_iter().map(Into::into).collect());
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

/// Ob die Adresse für „Auf Sendung“ die richtige Form hat (nicht, ob der Server antwortet).
#[uniffi::export]
pub fn on_air_url_valid(url: String) -> bool {
    core::onair::valid_url(&url)
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

/// Sprache der Texte aus dem Kern nach dem Sprachcode von macOS (`de`, `en` …);
/// Deutsch, eine eingereichte Sprache aus `locales.json`, sonst Englisch. Vor
/// dem ersten anderen Aufruf setzen.
#[uniffi::export]
pub fn set_language(code: String) {
    core::lang::set(core::lang::from_code(&code));
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
        None => Err(tr!("Server, Benutzer und App-Passwort eintragen", "Enter server, user and app password")),
    };
    match result {
        Ok(n) => NextcloudCheck { calendars: n as u32, error: None },
        Err(e) => NextcloudCheck { calendars: 0, error: Some(e) },
    }
}

/// Zugang zum MQTT-Broker prüfen: verbinden und abmelden (blockiert bis zu
/// einigen Sekunden, nicht im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn mqtt_check(host: String, port: u16, tls: bool, user: String, password: String) -> MqttCheck {
    let result = match mqtt_access(&host, port, tls, &user, password) {
        Some(m) => core::mqtt::check(&m),
        None => Err(tr!("Adresse des Brokers eintragen", "Enter the broker address")),
    };
    MqttCheck { error: result.err() }
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
        None => Err(tr!("Adresse des Pixoo eintragen", "Enter the Pixoo address")),
    };
    PixooCheck { error: result.err() }
}

/// AWTRIX-Uhr unter `host` erreichbar? (blockiert bis zu drei Sekunden, nicht
/// im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn awtrix_check(host: String) -> AwtrixCheck {
    let result = match core::pixoo::normalize(&host) {
        Some(h) => core::awtrix::check(&h),
        None => Err(tr!("Adresse der Uhr eintragen", "Enter the clock address")),
    };
    AwtrixCheck { error: result.err() }
}

/// Mit der Hue Bridge unter `host` koppeln; vorher den Knopf an der Bridge
/// drücken (blockiert bis zu einigen Sekunden, nicht im Haupt-Thread aufrufen)
#[uniffi::export]
pub fn hue_pair(host: String) -> HuePair {
    let result = match core::pixoo::normalize(&host) {
        Some(h) => core::hue::pair(&h),
        None => Err(tr!("Adresse der Bridge eintragen", "Enter the bridge address")),
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
        None => Err(tr!("Erst mit der Bridge koppeln", "Pair with the bridge first")),
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

/// WLED-Geräte per Bonjour suchen (blockiert etwa drei Sekunden, nicht im
/// Haupt-Thread aufrufen)
#[uniffi::export]
pub fn wled_discover() -> FoundDevices {
    found(core::wled::discover())
}

/// Mit der Nanoleaf-Lampe unter `host` koppeln; vorher den Einschaltknopf
/// 5–7 Sekunden halten (blockiert bis zu drei Sekunden, nicht im Haupt-Thread
/// aufrufen)
#[uniffi::export]
pub fn nanoleaf_pair(host: String) -> NanoleafPair {
    let result = match core::nanoleaf::normalize(&host) {
        Some(h) => core::nanoleaf::pair(&h),
        None => Err(tr!("Adresse der Nanoleaf-Lampe eintragen", "Enter the Nanoleaf light address")),
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
    fn nextcloud_status_needs_access_switch_and_text() {
        let nc = || nextcloud("https://cloud.example.org".into(), "frank".into(), "pw".into());
        let s = nextcloud_status(nc(), true, " Im Call ", " 🔴 ", true).unwrap();
        assert_eq!((s.text.as_str(), s.emoji.as_str(), s.dnd), ("Im Call", "🔴", true));
        assert!(nextcloud_status(nc(), false, "Im Call", "", false).is_none(), "Schalter aus");
        assert!(nextcloud_status(nc(), true, "  ", "🔴", false).is_none(), "ohne Text");
        let partial = nextcloud("https://cloud.example.org".into(), "frank".into(), " ".into());
        assert!(nextcloud_status(partial, true, "Im Call", "", false).is_none(), "ohne Passwort");
    }

    #[test]
    fn govee_hosts_are_cleaned_and_deduplicated() {
        let hosts = ["192.168.1.31", " ", "http://192.168.1.31/", "192.168.1.32"].map(String::from);
        let list: Vec<String> = govee(&hosts).into_iter().map(|g| g.host).collect();
        assert_eq!(list, ["192.168.1.31", "192.168.1.32"]);
    }

    const ALL: OutputStates = OutputStates { call: true, timer: true, focus: true };

    fn config() -> MicLedConfig {
        MicLedConfig {
            enabled: true,
            transport: TransportKind::NoLamp,
            luxafor_id: String::new(),
            mic_on_color: "ff0000".into(),
            mic_off_color: "00ff00".into(),
            after_mic: AfterMic::Restore,
            release_delay_ms: 3000,
            ignored: vec![],
            off_when_away: true,
            camera_enabled: true,
            camera_color: "ff00ff".into(),
            on_air_url: String::new(),
            reminder_color: "ffff00".into(),
            reminder_lead_s: 120,
            reminders_enabled: false,
            meeting_end_enabled: false,
            nextcloud_server: String::new(),
            nextcloud_user: String::new(),
            nextcloud_password: String::new(),
            nextcloud_calendar: false,
            nextcloud_status: false,
            nextcloud_status_text: String::new(),
            nextcloud_status_emoji: String::new(),
            nextcloud_status_dnd: false,
            app_colors: vec![],
            timer_color: "8000ff".into(),
            focus_enabled: false,
            focus_color: "ff8800".into(),
            dim_enabled: false,
            dim_from_min: 0,
            dim_to_min: 0,
            dim_percent: 100,
            slack_token: String::new(),
            slack_text: String::new(),
            slack_emoji: String::new(),
            pixoo_host: String::new(),
            pixoo_states: ALL,
            awtrix_host: String::new(),
            awtrix_states: ALL,
            hue_host: String::new(),
            hue_key: String::new(),
            hue_fingerprint: String::new(),
            hue_target_kind: HueTargetKind::Light,
            hue_target_id: String::new(),
            hue_states: ALL,
            govee_hosts: vec![],
            govee_states: ALL,
            nanoleaf: vec![],
            nanoleaf_states: ALL,
            wled_hosts: vec![],
            wled_states: ALL,
            robobloq: false,
            robobloq_states: ALL,
            mqtt_host: " homeassistant.local ".into(),
            mqtt_port: 0,
            mqtt_tls: false,
            mqtt_user: "micled".into(),
            mqtt_password: "pw".into(),
            mqtt_device_id: "a1b2c3d4e5f6".into(),
            mqtt_device_name: "Franks MacBook".into(),
        }
    }

    #[test]
    fn mqtt_settings_become_core_config() {
        let m = core::Config::from(config()).mqtt.expect("an");
        assert_eq!((m.host.as_str(), m.port, m.tls), ("homeassistant.local", 1883, false));
        assert_eq!((m.user.as_str(), m.password.as_str()), ("micled", "pw"));
        assert_eq!((m.device_id.as_str(), m.device_name.as_str()), ("a1b2c3d4e5f6", "Franks MacBook"));
        let tls = core::Config::from(MicLedConfig { mqtt_tls: true, ..config() }).mqtt.unwrap();
        assert_eq!(tls.port, 8883, "Standard-Port mit TLS");
        let own = core::Config::from(MicLedConfig { mqtt_port: 1884, mqtt_tls: true, ..config() }).mqtt.unwrap();
        assert_eq!(own.port, 1884);
        let in_host = core::Config::from(MicLedConfig { mqtt_host: "homeassistant.local:1885".into(), ..config() });
        let m = in_host.mqtt.unwrap();
        assert_eq!((m.host.as_str(), m.port), ("homeassistant.local", 1885), "Port in der Adresse gewinnt");
    }

    #[test]
    fn mqtt_without_host_or_id_is_off() {
        assert!(core::Config::from(MicLedConfig { mqtt_host: " ".into(), ..config() }).mqtt.is_none());
        assert!(core::Config::from(MicLedConfig { mqtt_device_id: "/#".into(), ..config() }).mqtt.is_none());
    }

    #[test]
    fn mqtt_check_needs_host() {
        assert_eq!(mqtt_check(" ".into(), 0, false, String::new(), String::new()).error.as_deref(), Some("Adresse des Brokers eintragen"));
    }

    #[test]
    fn wled_hosts_are_cleaned_and_deduplicated() {
        let hosts = ["10.0.0.7", " ", "http://10.0.0.7/", "10.0.0.7:80", "10.0.0.7:8080", "10.0.0.8"].map(String::from);
        let list: Vec<String> = wled(&hosts).into_iter().map(|w| w.host).collect();
        assert_eq!(list, ["10.0.0.7", "10.0.0.7:8080", "10.0.0.8"]);
    }

    #[test]
    fn nanoleaf_without_address_or_key_is_dropped() {
        let d = |host: &str, key: &str| NanoleafDevice { host: host.into(), key: key.into() };
        let list = nanoleaf(&[d("10.0.0.4", "k1"), d("10.0.0.4:16021", "k2"), d("10.0.0.5", " "), d("", "k3")]);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].host.as_str(), list[0].key.as_str()), ("10.0.0.4:16021", "k1"));
    }
}
