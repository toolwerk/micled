//! Hintergrund-Thread: fragt Mikrofon und Statusdatei ab, schaltet die Lampe
//! und meldet Änderungen an die Oberfläche.

use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::color::Rgb;
use crate::engine::{Config, Engine};
use crate::lamp::{self, Lamp};
use crate::mic::{self, MicUser};
use crate::shared;

const POLL: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub mic_users: Vec<MicUser>,
    /// Mikrofarbe ist gerade aktiv
    pub overriding: bool,
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
    Stop,
}

pub struct Monitor {
    tx: Sender<Cmd>,
    status: Arc<Mutex<Status>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Monitor {
    /// Startet die Überwachung. `state_path` = `None` nimmt den Standardpfad.
    pub fn start(cfg: Config, state_path: Option<PathBuf>, listener: Arc<dyn StatusListener>) -> Monitor {
        let path = state_path.or_else(shared::default_path);
        let base = path.as_ref().and_then(shared::read).map(|s| s.color);
        let status = Arc::new(Mutex::new(Status {
            enabled: cfg.enabled,
            mic_users: vec![],
            overriding: false,
            base,
            shown: base,
            error: None,
        }));
        let (tx, rx) = channel::<Cmd>();
        let st = status.clone();

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
                };
                w.file_mtime = w.mtime();
                loop {
                    match rx.recv_timeout(POLL) {
                        Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Ok(Cmd::Config(c)) => w.set_config(c),
                        Ok(Cmd::Color(c)) => w.user_color(c),
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
            })
            .expect("Monitor-Thread");

        Monitor { tx, status, thread: Mutex::new(Some(thread)) }
    }

    pub fn update_config(&self, cfg: Config) {
        let _ = self.tx.send(Cmd::Config(cfg));
    }

    /// Farbe direkt setzen (wird zur neuen ruhenden Farbe)
    pub fn set_color(&self, c: Rgb) {
        let _ = self.tx.send(Cmd::Color(c));
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
}

impl Worker {
    fn mtime(&self) -> Option<SystemTime> {
        self.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
    }

    fn set_config(&mut self, cfg: Config) {
        if cfg.transport != self.engine.config().transport {
            self.lamp = lamp::open(&cfg.transport);
            self.last_try = None;
        }
        if let Some(c) = self.engine.set_config(cfg) {
            self.pending = Some(c);
        }
    }

    fn user_color(&mut self, c: Rgb) {
        let c = self.engine.on_user_color(c);
        self.pending = Some(c);
        self.last_try = None;
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
                    self.lamp_error = None;
                }
            }
        }

        // 2. Mikrofon
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
        if let Some(c) = self.engine.on_mic(users, Instant::now()) {
            self.pending = Some(c);
            self.last_try = None;
        }

        // 3. Lampe schalten (mit Wiederholung, falls sie gerade fehlt)
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
        }
    }

    fn status(&self) -> Status {
        Status {
            enabled: self.engine.config().enabled,
            mic_users: self.engine.users().to_vec(),
            overriding: self.engine.overriding(),
            base: self.engine.base(),
            shown: self.shown,
            error: self.lamp_error.clone().or_else(|| self.mic_error.clone()),
        }
    }
}
