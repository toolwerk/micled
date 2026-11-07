//! Gemeinsamer Hintergrund-Thread für Ausgaben im eigenen Netz (Pixoo 64,
//! Philips Hue): Ein fehlendes Gerät hält die Lampe nicht auf, Fehler landen
//! im Fehler-Slot und werden nach einer Pause wiederholt.

use std::marker::PhantomData;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::display::Scene;

/// Ein Gerät, das eine [`Scene`] zeigt und danach seinen alten Zustand
/// wiederherstellt
pub trait Device: Default + Send + 'static {
    /// Ziel in den Einstellungen (Adresse, Zugang, Lampe …)
    type Cfg: Clone + PartialEq + Send + 'static;

    /// Zeigt `scene` auf `cfg` (`None` = aus). Wechselt das Ziel, wird das
    /// alte vorher zurückgestellt, so gut es geht.
    fn apply(&mut self, cfg: Option<&Self::Cfg>, scene: &Scene) -> Result<(), String>;

    /// Stellt den Zustand von vorher wieder her, falls etwas von MicLED zu sehen ist
    fn restore(&mut self) -> Result<(), String>;

    /// Ziel, auf das zuletzt angewandt wurde
    fn current(&self) -> Option<&Self::Cfg>;
}

enum Msg<C> {
    Update(Option<C>, Scene),
    /// Zustand zurückstellen und bestätigen (beim Beenden der App)
    Shutdown(Sender<()>),
}

/// Wartezeit nach einer Änderung des Ziels (Tippen nicht senden)
const SETTLE: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_secs(30);

/// Schickt Szenen in einem eigenen Thread an ein [`Device`]. Ein Erfolg leert
/// den Fehler-Slot.
pub struct Updater<D: Device> {
    tx: Sender<Msg<D::Cfg>>,
    _device: PhantomData<fn() -> D>,
}

impl<D: Device> Updater<D> {
    pub fn start(name: &str, error: Arc<Mutex<Option<String>>>) -> Updater<D> {
        let (tx, rx) = channel::<Msg<D::Cfg>>();
        std::thread::Builder::new()
            .name(format!("micled-{name}"))
            .spawn(move || {
                let mut device = D::default();
                let mut wanted: (Option<D::Cfg>, Scene) = (None, Scene::Idle);
                let mut dirty = false;
                let shutdown = |device: &mut D, ack: Sender<()>| {
                    if let Err(e) = device.restore() {
                        log::warn!("{e}");
                    }
                    let _ = ack.send(());
                };
                loop {
                    let wait = match (dirty, wanted.0.as_ref() != device.current()) {
                        (false, _) => Duration::from_secs(3600),
                        (true, true) => SETTLE,
                        (true, false) => Duration::ZERO,
                    };
                    match rx.recv_timeout(wait) {
                        Ok(Msg::Update(cfg, scene)) => {
                            wanted = (cfg, scene);
                            dirty = true;
                            continue;
                        }
                        Ok(Msg::Shutdown(ack)) => {
                            shutdown(&mut device, ack);
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) if !dirty => continue,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    let r = device.apply(wanted.0.as_ref(), &wanted.1);
                    if let Err(e) = &r {
                        log::warn!("{e}");
                    }
                    dirty = r.is_err();
                    *error.lock().unwrap() = r.err();
                    if dirty {
                        // nach einer Pause erneut versuchen, neue Wechsel kommen vorher an
                        match rx.recv_timeout(RETRY) {
                            Ok(Msg::Update(cfg, scene)) => wanted = (cfg, scene),
                            Ok(Msg::Shutdown(ack)) => {
                                shutdown(&mut device, ack);
                                break;
                            }
                            Err(RecvTimeoutError::Disconnected) => break,
                            Err(RecvTimeoutError::Timeout) => {}
                        }
                    }
                }
            })
            .expect("Ausgabe-Thread");
        Updater { tx, _device: PhantomData }
    }

    /// Gewünschter Zustand: Ziel (`None` = aus) und Szene
    pub fn send(&self, cfg: Option<D::Cfg>, scene: Scene) {
        let _ = self.tx.send(Msg::Update(cfg, scene));
    }

    /// Zurückstellen anstoßen, ohne zu warten. Die Rückgabe meldet das Ende
    /// (`None`, wenn der Thread schon beendet ist).
    pub fn begin_shutdown(&self) -> Option<Receiver<()>> {
        let (ack, done) = channel();
        self.tx.send(Msg::Shutdown(ack)).ok().map(|()| done)
    }

    /// Zustand zurückstellen und höchstens `timeout` darauf warten
    pub fn shutdown(&self, timeout: Duration) {
        if let Some(done) = self.begin_shutdown() {
            let _ = done.recv_timeout(timeout);
        }
    }
}
