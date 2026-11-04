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
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterMic {
    /// Zur Farbe von vor dem Mikrofon zurück
    Restore,
    /// Immer die „Mikro aus“-Farbe
    Fixed,
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
    /// Mikrofarbe ist gerade aktiv
    pub overriding: bool,
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

impl From<MicLedConfig> for core::Config {
    fn from(c: MicLedConfig) -> Self {
        core::Config {
            enabled: c.enabled,
            transport: match c.transport {
                TransportKind::Usb => core::Transport::Usb,
                TransportKind::Webhook => core::Transport::Webhook { luxafor_id: c.luxafor_id },
            },
            mic_on_color: color(&c.mic_on_color, Rgb::RED),
            mic_off_color: color(&c.mic_off_color, Rgb::GREEN),
            after_mic: match c.after_mic {
                AfterMic::Restore => core::AfterMic::Restore,
                AfterMic::Fixed => core::AfterMic::Fixed,
            },
            release_delay: Duration::from_millis(c.release_delay_ms as u64),
            ignored: c.ignored,
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
            overriding: s.overriding,
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

/// Aktuelle Mikrofon-Nutzer (für die Ignorieren-Liste in den Einstellungen)
#[uniffi::export]
pub fn current_mic_users() -> Vec<MicUser> {
    core::mic::users().unwrap_or_default().into_iter().map(Into::into).collect()
}
