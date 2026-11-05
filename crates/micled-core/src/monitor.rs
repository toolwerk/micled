//! Hintergrund-Thread: fragt Mikrofon, Kamera und Statusdatei ab, schaltet die
//! Lampe, meldet „Auf Sendung“ nach außen und Änderungen an die Oberfläche.

use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::color::Rgb;
use crate::engine::{Away, Config, Engine};
use crate::lamp::{self, Lamp, Transport};
use crate::mic::{self, MicUser};
use crate::reminder::Reminders;
use crate::stats::CallLog;
use crate::{camera, onair, shared};

const POLL: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(2);
/// Wechsel bei der Termin-Vorwarnung (je Abfrage einer), gerade Anzahl
const FLASH_STEPS: u8 = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub mic_users: Vec<MicUser>,
    /// Kamera in Benutzung (nur, wenn die Kamera ausgewertet wird)
    pub camera: bool,
    /// Mikro- bzw. Kamerafarbe ist gerade aktiv
    pub overriding: bool,
    /// Auf Sendung (Mikro oder Kamera, kurze Pausen eingeschlossen)
    pub live: bool,
    /// Farbe, zu der nach dem Mikrofon zurückgeschaltet wird (`None` = unbekannt)
    pub base: Option<Rgb>,
    /// Zuletzt erfolgreich gesetzte bzw. von anderer Stelle gemeldete Farbe
    pub shown: Option<Rgb>,
    pub error: Option<String>,
}

pub trait StatusListener: Send + Sync {
    fn status_changed(&self, status: Status);
}

enum Cmd {
    Config(Config),
    Color(Rgb),
    Away(Away, bool),
    Upcoming(Vec<u64>),
    Stop,
}

pub struct Monitor {
    tx: Sender<Cmd>,
    status: Arc<Mutex<Status>>,
    calls: Arc<Mutex<CallLog>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Monitor {
    /// Startet die Überwachung. `state_path` = `None` nimmt den Standardpfad.
    pub fn start(cfg: Config, state_path: Option<PathBuf>, listener: Arc<dyn StatusListener>) -> Monitor {
        let path = state_path.or_else(shared::default_path);
        let base = path.as_ref().and_then(shared::read).map(|s| s.color);
        let status = Arc::new(Mutex::new(Status {
            enabled: cfg.enabled,
            mic_users: vec![],
            camera: false,
            overriding: false,
            live: false,
            base,
            shown: base,
            error: None,
        }));
        let calls_path = CallLog::default_path();
        let calls = Arc::new(Mutex::new(calls_path.as_deref().map(CallLog::load).unwrap_or_default()));
        let (tx, rx) = channel::<Cmd>();
        let st = status.clone();
        let log = calls.clone();
        let thread = std::thread::Builder::new()
            .name("micled-monitor".into())
            .spawn(move || {
                let mut w = Worker {
                    lamp: lamp::open(&cfg.transport),
                    engine: Engine::new(cfg, base),
                    path,
                    file_mtime: None,
                    pending: None,
                    last_try: None,
                    shown: base,
                    lamp_error: None,
                    mic_error: None,
                    reminders: Reminders::default(),
                    flash_left: 0,
                    live: false,
                    camera_reported: false,
                    calls: log,
                    calls_path,
                    on_air_error: Arc::new(Mutex::new(None)),
                };
                w.file_mtime = w.mtime();
                loop {
                    match rx.recv_timeout(POLL) {
                        Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Ok(Cmd::Config(c)) => w.set_config(c),
                        Ok(Cmd::Color(c)) => w.user_color(c),
                        Ok(Cmd::Away(r, on)) => w.set_away(r, on),
                        Ok(Cmd::Upcoming(s)) => w.reminders.set(s),
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    w.tick();
                    let next = w.status();
                    let changed = {
                        let mut cur = st.lock().unwrap();
                        let changed = *cur != next;
                        *cur = next.clone();
                        changed
                    };
                    if changed {
                        listener.status_changed(next);
                    }
                }
                w.set_live(false);
            })
            .expect("Monitor-Thread");
        Monitor { tx, status, calls, thread: Mutex::new(Some(thread)) }
    }

    pub fn update_config(&self, cfg: Config) {
        let _ = self.tx.send(Cmd::Config(cfg));
    }

    /// Farbe direkt setzen (wird zur neuen ruhenden Farbe)
    pub fn set_color(&self, c: Rgb) {
        let _ = self.tx.send(Cmd::Color(c));
    }

    /// Ruhezustand, Bildschirm aus oder Sperre beginnt (`on = true`) bzw. endet
    pub fn set_away(&self, reason: Away, on: bool) {
        let _ = self.tx.send(Cmd::Away(reason, on));
    }

    /// Startzeiten der nächsten Termine (Unix-Sekunden) für die Vorwarnung
    pub fn set_upcoming(&self, starts: Vec<u64>) {
        let _ = self.tx.send(Cmd::Upcoming(starts));
    }

    /// Sekunden auf Sendung seit `since` (Unix-Sekunden), laufender Call eingeschlossen
    pub fn live_seconds_since(&self, since: u64) -> u64 {
        self.calls.lock().unwrap().total_since(since, unix_now())
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Worker {
    lamp: Box<dyn Lamp>,
    engine: Engine,
    path: Option<PathBuf>,
    file_mtime: Option<SystemTime>,
    /// Farbe, die noch gesetzt werden muss (z. B. weil die Lampe fehlte)
    pending: Option<Rgb>,
    last_try: Option<Instant>,
    shown: Option<Rgb>,
    lamp_error: Option<String>,
    mic_error: Option<String>,
    reminders: Reminders,
    /// Verbleibende Wechsel der Termin-Vorwarnung
    flash_left: u8,
    /// Zuletzt nach außen gemeldeter Zustand „Auf Sendung“
    live: bool,
    camera_reported: bool,
    calls: Arc<Mutex<CallLog>>,
    calls_path: Option<PathBuf>,
    on_air_error: Arc<Mutex<Option<String>>>,
}

impl Worker {
    fn mtime(&self) -> Option<SystemTime> {
        self.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
    }

    fn push(&mut self, c: Rgb) {
        self.pending = Some(c);
        self.last_try = None;
        self.flash_left = 0;
    }

    fn set_config(&mut self, cfg: Config) {
        if cfg.transport != self.engine.config().transport {
            self.lamp = lamp::open(&cfg.transport);
            self.last_try = None;
        }
        let url_changed = cfg.on_air_url != self.engine.config().on_air_url;
        if let Some(c) = self.engine.set_config(cfg) {
            self.push(c);
        }
        if url_changed {
            *self.on_air_error.lock().unwrap() = None;
        }
    }

    fn set_away(&mut self, reason: Away, on: bool) {
        if let Some(c) = self.engine.set_away(reason, on) {
            self.push(c);
        }
    }

    fn user_color(&mut self, c: Rgb) {
        let c = self.engine.on_user_color(c);
        self.push(c);
        if let Some(p) = &self.path {
            if let Err(e) = shared::write(p, c) {
                log::warn!("Statusdatei nicht schreibbar: {e}");
            }
            self.file_mtime = self.mtime();
        }
    }

    fn tick(&mut self) {
        // 1. Hat ein anderes Programm (Stream Deck) eine Farbe eingetragen?
        let mtime = self.mtime();
        if mtime != self.file_mtime {
            self.file_mtime = mtime;
            if let Some(s) = self.path.as_ref().and_then(shared::read) {
                if s.source != shared::SOURCE {
                    self.engine.on_shared_color(s.color);
                    self.shown = Some(s.color);
                    // eine noch ausstehende ältere Farbe nicht mehr nachschieben
                    self.pending = None;
                    self.flash_left = 0;
                    self.lamp_error = None;
                }
            }
        }

        // 2. Mikrofon und Kamera
        let users = match mic::users() {
            Ok(u) => {
                self.mic_error = None;
                u
            }
            Err(e) => {
                self.mic_error = Some(e);
                vec![]
            }
        };
        let cam = self.engine.config().camera && camera::in_use().unwrap_or(false);
        if let Some(c) = self.engine.on_input(users, cam, Instant::now()) {
            self.push(c);
        }

        // 3. Termin-Vorwarnung (nur in Ruhe und per USB, der Webhook ist zu träge)
        let lead = self.engine.config().reminder_lead.as_secs();
        if self.reminders.due(unix_now(), lead)
            && !self.engine.overriding()
            && !self.engine.dark()
            && self.pending.is_none()
            && self.engine.config().transport == Transport::Usb
        {
            self.flash_left = FLASH_STEPS;
        }

        // 4. Lampe schalten (mit Wiederholung, falls sie gerade fehlt)
        if let Some(c) = self.pending {
            let due = self.last_try.map(|t| t.elapsed() >= RETRY).unwrap_or(true);
            if due {
                self.last_try = Some(Instant::now());
                match self.lamp.set(c) {
                    Ok(()) => {
                        log::info!("Lampe auf {c}");
                        self.pending = None;
                        self.shown = Some(c);
                        self.lamp_error = None;
                    }
                    Err(e) => {
                        log::warn!("{e}");
                        self.lamp_error = Some(e.to_string());
                    }
                }
            }
        } else if self.flash_left > 0 {
            self.flash_left -= 1;
            let c = if self.flash_left % 2 == 1 {
                self.engine.config().reminder_color
            } else {
                self.shown.unwrap_or(Rgb::OFF)
            };
            if let Err(e) = self.lamp.set(c) {
                log::warn!("Vorwarnung: {e}");
                self.flash_left = 0;
            }
        }

        // 5. Auf Sendung: Statistik und Meldung nach außen
        let live = self.engine.live();
        if live != self.live || (live && self.engine.camera_on() != self.camera_reported) {
            self.set_live(live);
        }
    }

    fn set_live(&mut self, live: bool) {
        if live != self.live {
            let mut calls = self.calls.lock().unwrap();
            if live {
                calls.start(unix_now());
            } else {
                calls.stop(unix_now());
                if let Some(p) = &self.calls_path {
                    if let Err(e) = calls.save(p) {
                        log::warn!("Statistik nicht schreibbar: {e}");
                    }
                }
            }
        }
        self.live = live;
        self.camera_reported = live && self.engine.camera_on();

        let url = self.engine.config().on_air_url.trim().to_string();
        if url.is_empty() {
            return;
        }
        let apps: Vec<String> = self
            .engine
            .users()
            .iter()
            .map(|u| if u.bundle_id.is_empty() { u.name.clone() } else { u.bundle_id.clone() })
            .collect();
        let body = onair::payload(live, self.camera_reported, &apps);
        let err = self.on_air_error.clone();
        // eigener Thread, damit ein langsamer Server die Lampe nicht aufhält
        std::thread::spawn(move || {
            let r = onair::send(&url, &body).err();
            if let Some(e) = &r {
                log::warn!("{e}");
            }
            *err.lock().unwrap() = r;
        });
    }

    fn status(&self) -> Status {
        Status {
            enabled: self.engine.config().enabled,
            mic_users: self.engine.users().to_vec(),
            camera: self.engine.camera_on(),
            overriding: self.engine.overriding(),
            live: self.live,
            base: self.engine.base(),
            shown: self.shown,
            error: self
                .lamp_error
                .clone()
                .or_else(|| self.mic_error.clone())
                .or_else(|| self.on_air_error.lock().unwrap().clone()),
        }
    }
}
