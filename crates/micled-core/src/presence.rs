//! Gemeinsamer Ablauf für einen Status „Im Call“ bei einem Dienst (Slack,
//! Nextcloud): setzen, solange du auf Sendung bist, danach zurückstellen.
//!
//! Der Dienst selbst steckt in einem [`Presence`]; der [`Updater`] arbeitet
//! die Wechsel in einem eigenen Thread ab, damit das Netz die Lampe nicht
//! aufhält und sich Setzen und Zurücksetzen nicht überholen.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Zustand eines Dienstes: was MicLED gesetzt hat und was vorher da war
pub trait Presence: Default + Send + 'static {
    /// Einstellung (Zugang, Text, Emoji)
    type Cfg: Clone + PartialEq + Send + 'static;

    /// Gewünschten Zustand herstellen. `startup`: einen nach einem Absturz
    /// hängengebliebenen eigenen Status aufräumen.
    fn apply(&mut self, wanted: Option<&Self::Cfg>, live: bool, now: u64, startup: bool) -> Result<(), String>;
}

/// Wie der gewünschte Zustand erreicht wird: zuerst den eigenen Status mit den
/// Zugangsdaten zurücksetzen, mit denen er gesetzt wurde, dann ggf. neu setzen.
/// `ours` = Einstellung, mit der MicLED gerade einen Status gesetzt hat.
pub fn plan<C: Clone + PartialEq>(ours: Option<&C>, wanted: Option<&C>, live: bool) -> (Option<C>, Option<C>) {
    let target = wanted.filter(|_| live);
    let keep = matches!((ours, target), (Some(o), Some(t)) if o == t);
    let restore = ours.filter(|_| !keep).cloned();
    let set = target.filter(|_| !keep).cloned();
    (restore, set)
}

enum Msg<C> {
    Update(Option<C>, bool),
    /// Zurücksetzen und bestätigen (beim Beenden der App)
    Shutdown(Sender<()>),
}

/// Arbeitet Wechsel nacheinander in einem eigenen Thread ab. Änderungen der
/// Einstellungen (Tippen) werden kurz gesammelt, Fehler nach einer Pause
/// wiederholt, z. B. nach dem Aufwachen ohne Netz.
pub struct Updater<P: Presence> {
    tx: Sender<Msg<P::Cfg>>,
}

const SETTLE: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_secs(30);

impl<P: Presence> Updater<P> {
    /// `name` benennt den Thread (`micled-<name>`), `error` nimmt den letzten Fehler auf
    pub fn start(name: &str, error: Arc<Mutex<Option<String>>>) -> Updater<P> {
        let (tx, rx) = channel::<Msg<P::Cfg>>();
        std::thread::Builder::new()
            .name(format!("micled-{name}"))
            .spawn(move || run::<P>(rx, error))
            .expect("Status-Thread");
        Updater { tx }
    }

    /// Gewünschter Zustand: Einstellung (`None` = aus) und „Auf Sendung“
    pub fn send(&self, cfg: Option<P::Cfg>, live: bool) {
        let _ = self.tx.send(Msg::Update(cfg, live));
    }

    /// Eigenen Status zurücksetzen; der Empfänger meldet, wenn das erledigt ist
    pub fn begin_shutdown(&self) -> Option<Receiver<()>> {
        let (ack, done) = channel();
        self.tx.send(Msg::Shutdown(ack)).ok().map(|()| done)
    }
}

fn run<P: Presence>(rx: Receiver<Msg<P::Cfg>>, error: Arc<Mutex<Option<String>>>) {
    let mut st = P::default();
    let mut wanted: (Option<P::Cfg>, bool) = (None, false);
    let mut dirty = false;
    let mut startup = true;
    let shutdown = |st: &mut P, ack: Sender<()>| {
        let _ = st.apply(None, false, unix_now(), false);
        let _ = ack.send(());
    };
    loop {
        let wait = if dirty { SETTLE } else { Duration::from_secs(3600) };
        match rx.recv_timeout(wait) {
            Ok(Msg::Update(cfg, live)) => {
                wanted = (cfg, live);
                dirty = true;
                continue;
            }
            Ok(Msg::Shutdown(ack)) => {
                shutdown(&mut st, ack);
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) if !dirty => continue,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let r = st.apply(wanted.0.as_ref(), wanted.1, unix_now(), startup);
        if let Err(e) = &r {
            log::warn!("{e}");
        }
        dirty = r.is_err();
        *error.lock().unwrap() = r.clone().err();
        if r.is_ok() {
            startup = false;
        } else {
            // nach einer Pause erneut versuchen, neue Wechsel kommen vorher an
            if let Ok(m) = rx.recv_timeout(RETRY) {
                match m {
                    Msg::Update(cfg, live) => wanted = (cfg, live),
                    Msg::Shutdown(ack) => {
                        shutdown(&mut st, ack);
                        break;
                    }
                }
            }
        }
    }
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_sets_and_restores() {
        let a = "Im Call";
        assert_eq!(plan(None, Some(&a), true), (None, Some(a)));
        assert_eq!(plan(Some(&a), Some(&a), true), (None, None), "schon gesetzt");
        assert_eq!(plan(Some(&a), Some(&a), false), (Some(a), None));
        assert_eq!(plan(None, Some(&a), false), (None, None));
    }

    #[test]
    fn plan_restores_with_old_settings() {
        let (a, b) = ("Im Call", "In Meeting");
        assert_eq!(plan(Some(&a), Some(&b), true), (Some(a), Some(b)), "Text im Call geändert");
        assert_eq!(plan(Some(&a), None, true), (Some(a), None), "im Call ausgeschaltet");
    }

    /// Zählt die Aufrufe, damit sich der Thread ohne Netz prüfen lässt
    #[derive(Default)]
    struct Probe;

    static CALLS: Mutex<Vec<(Option<&'static str>, bool, bool)>> = Mutex::new(Vec::new());

    impl Presence for Probe {
        type Cfg = &'static str;
        fn apply(&mut self, wanted: Option<&&'static str>, live: bool, _now: u64, startup: bool) -> Result<(), String> {
            CALLS.lock().unwrap().push((wanted.copied(), live, startup));
            Ok(())
        }
    }

    #[test]
    fn updater_applies_last_wish_and_resets_on_shutdown() {
        let error = Arc::new(Mutex::new(None));
        let u = Updater::<Probe>::start("probe", error.clone());
        u.send(Some("Im C"), true);
        u.send(Some("Im Call"), true);
        std::thread::sleep(SETTLE + Duration::from_millis(500));
        u.begin_shutdown().unwrap().recv_timeout(Duration::from_secs(2)).unwrap();
        let calls = CALLS.lock().unwrap().clone();
        assert_eq!(calls, vec![(Some("Im Call"), true, true), (None, false, false)]);
        assert_eq!(*error.lock().unwrap(), None);
    }
}
