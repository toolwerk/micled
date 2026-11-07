//! Gemeinsamer Hintergrund-Thread für Ausgaben im eigenen Netz (Pixoo 64,
//! Philips Hue, Govee, Nanoleaf): Ein fehlendes Gerät hält die Lampe nicht
//! auf, Fehler landen im Fehler-Slot und werden nach einer Pause wiederholt.
//! [`Fleet`] steuert mehrere Geräte derselben Art als ein [`Device`].

use std::marker::PhantomData;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::color::Rgb;
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

/// Farbe einer Szene, `None` bei `Idle`
pub(crate) fn scene_color(scene: &Scene) -> Option<Rgb> {
    match scene {
        Scene::Idle => None,
        Scene::Live { color, .. } | Scene::Timer { color, .. } | Scene::Focus { color } => Some(*color),
    }
}

/// Ziel mit eigener Adresse, über die eine [`Fleet`] ihre Geräte zuordnet
pub trait Keyed {
    fn address(&self) -> &str;
}

/// So oft wird ein entferntes Gerät zurückgestellt, danach aufgegeben (wie
/// `MAX_ORPHAN_TRIES` bei Hue)
const MAX_LEAVING_TRIES: u32 = 5;

/// Entferntes Gerät, das noch zurückgestellt werden muss
struct Leaving<S> {
    key: String,
    device: S,
    tries: u32,
}

/// Mehrere Geräte derselben Art als ein [`Device`]: Jedes zeigt die Szene für
/// sich, ein Fehler hält die anderen nicht auf. Ein aus der Liste entferntes
/// Gerät wird zurückgestellt, bei einem Fehler bis zu fünfmal.
///
/// Die Geräte melden einen Fehler beim Zurückstellen mit `Err` und behalten
/// dann ihren Zustand, damit ein weiterer Versuch möglich ist.
pub struct Fleet<S: Device>
where
    S::Cfg: Keyed,
{
    devices: Vec<(String, S)>,
    leaving: Vec<Leaving<S>>,
    current: Option<Vec<S::Cfg>>,
}

impl<S: Device> Default for Fleet<S>
where
    S::Cfg: Keyed,
{
    fn default() -> Self {
        Fleet { devices: Vec::new(), leaving: Vec::new(), current: None }
    }
}

impl<S: Device> Fleet<S>
where
    S::Cfg: Keyed,
{
    /// Entfernte Geräte zurückstellen; Fehler kommen nach `errors`
    fn retry_leaving(&mut self, errors: &mut Vec<String>) {
        for mut l in std::mem::take(&mut self.leaving) {
            match l.device.apply(None, &Scene::Idle) {
                Ok(()) => {}
                Err(e) if l.tries + 1 >= MAX_LEAVING_TRIES => {
                    log::warn!("entferntes Gerät {} aufgegeben: {e}", l.key);
                }
                Err(e) => {
                    l.tries += 1;
                    errors.push(e);
                    self.leaving.push(l);
                }
            }
        }
    }
}

fn joined(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

impl<S: Device> Device for Fleet<S>
where
    S::Cfg: Keyed,
{
    type Cfg = Vec<S::Cfg>;

    fn apply(&mut self, cfg: Option<&Vec<S::Cfg>>, scene: &Scene) -> Result<(), String> {
        let wanted: &[S::Cfg] = cfg.map(Vec::as_slice).unwrap_or(&[]);
        let mut old = std::mem::take(&mut self.devices);
        // nicht mehr gewünschte Geräte gehen zum Zurückstellen
        for (key, device) in old.extract_if(.., |(k, _)| !wanted.iter().any(|w| w.address() == k)) {
            self.leaving.push(Leaving { key, device, tries: 0 });
        }
        let mut errors = Vec::new();
        for w in wanted {
            let key = w.address().to_string();
            if self.devices.iter().any(|(k, _)| *k == key) {
                continue; // doppelte Adresse nur einmal
            }
            let device = match old.iter().position(|(k, _)| *k == key) {
                Some(i) => old.swap_remove(i).1,
                // wieder hinzugefügt: Zustand von vorher bleibt erhalten
                None => match self.leaving.iter().position(|l| l.key == key) {
                    Some(i) => self.leaving.swap_remove(i).device,
                    None => S::default(),
                },
            };
            self.devices.push((key, device));
            let (_, d) = self.devices.last_mut().expect("gerade eingefügt");
            if let Err(e) = d.apply(Some(w), scene) {
                errors.push(e);
            }
        }
        self.current = cfg.cloned();
        self.retry_leaving(&mut errors);
        joined(errors)
    }

    fn restore(&mut self) -> Result<(), String> {
        let mut errors: Vec<String> = self.devices.iter_mut().filter_map(|(_, d)| d.restore().err()).collect();
        self.retry_leaving(&mut errors);
        joined(errors)
    }

    fn current(&self) -> Option<&Vec<S::Cfg>> {
        self.current.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, PartialEq)]
    struct Cfg(&'static str);

    impl Keyed for Cfg {
        fn address(&self) -> &str {
            self.0
        }
    }

    type Log = Arc<Mutex<Vec<String>>>;

    thread_local! {
        /// Befehle aller Fake-Geräte und Adressen, die gerade scheitern
        static STATE: (Log, Arc<Mutex<Vec<&'static str>>>) = Default::default();
    }

    fn log() -> Vec<String> {
        STATE.with(|s| std::mem::take(&mut *s.0.lock().unwrap()))
    }

    fn fail(hosts: &[&'static str]) {
        STATE.with(|s| *s.1.lock().unwrap() = hosts.to_vec());
    }

    /// Gerät wie die echten: Fehler beim Zurückstellen behält den Zustand
    #[derive(Default)]
    struct Fake {
        cfg: Option<Cfg>,
        shown: bool,
    }

    impl Device for Fake {
        type Cfg = Cfg;

        fn apply(&mut self, cfg: Option<&Cfg>, scene: &Scene) -> Result<(), String> {
            let target = cfg.or(self.cfg.as_ref()).cloned().expect("Ziel");
            if STATE.with(|s| s.1.lock().unwrap().contains(&target.0)) {
                return Err(format!("{} antwortet nicht", target.0));
            }
            let show = cfg.is_some() && *scene != Scene::Idle;
            if show != self.shown {
                STATE.with(|s| s.0.lock().unwrap().push(format!("{} {}", target.0, if show { "Farbe" } else { "zurück" })));
            }
            self.shown = show;
            self.cfg = cfg.cloned();
            Ok(())
        }

        fn restore(&mut self) -> Result<(), String> {
            match self.cfg.clone() {
                Some(c) => self.apply(Some(&c), &Scene::Idle),
                None => Ok(()),
            }
        }

        fn current(&self) -> Option<&Cfg> {
            self.cfg.as_ref()
        }
    }

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED };

    fn cfg(hosts: &[&'static str]) -> Vec<Cfg> {
        hosts.iter().map(|h| Cfg(h)).collect()
    }

    #[test]
    fn one_failing_device_does_not_stop_the_others() {
        let mut f = Fleet::<Fake>::default();
        fail(&["b"]);
        let err = f.apply(Some(&cfg(&["a", "b", "c"])), &LIVE).unwrap_err();
        assert_eq!(err, "b antwortet nicht");
        assert_eq!(log(), ["a Farbe", "c Farbe"]);
        fail(&[]);
        f.apply(Some(&cfg(&["a", "b", "c"])), &LIVE).unwrap();
        assert_eq!(log(), ["b Farbe"], "nur b nachgeholt");
        fail(&["a", "c"]);
        let err = f.apply(Some(&cfg(&["a", "b", "c"])), &Scene::Idle).unwrap_err();
        assert_eq!(err, "a antwortet nicht; c antwortet nicht", "alle Meldungen");
        assert_eq!(log(), ["b zurück"]);
        fail(&[]);
    }

    #[test]
    fn removed_device_is_restored() {
        let mut f = Fleet::<Fake>::default();
        f.apply(Some(&cfg(&["a", "b"])), &LIVE).unwrap();
        log();
        f.apply(Some(&cfg(&["b"])), &LIVE).unwrap();
        assert_eq!(log(), ["a zurück"]);
        assert_eq!(f.current(), Some(&cfg(&["b"])));
        f.apply(None, &LIVE).unwrap();
        assert_eq!(log(), ["b zurück"]);
        assert_eq!(f.current(), None);
    }

    #[test]
    fn failed_restore_of_removed_device_is_retried_then_given_up() {
        let mut f = Fleet::<Fake>::default();
        f.apply(Some(&cfg(&["a", "b"])), &LIVE).unwrap();
        log();
        fail(&["a"]);
        assert_eq!(f.apply(Some(&cfg(&["b"])), &LIVE).unwrap_err(), "a antwortet nicht");
        assert_eq!(f.leaving.len(), 1);
        fail(&[]);
        f.apply(Some(&cfg(&["b"])), &LIVE).unwrap();
        assert_eq!(log(), ["a zurück"], "beim nächsten Durchgang nachgeholt");
        assert!(f.leaving.is_empty());

        f.apply(Some(&cfg(&["a", "b"])), &LIVE).unwrap();
        log();
        fail(&["a"]);
        let errors = (0..MAX_LEAVING_TRIES + 2).filter(|_| f.apply(Some(&cfg(&["b"])), &LIVE).is_err()).count();
        assert_eq!(errors as u32, MAX_LEAVING_TRIES - 1, "nach fünf Versuchen aufgegeben");
        assert!(f.leaving.is_empty());
        fail(&[]);
    }

    #[test]
    fn re_added_device_keeps_its_state() {
        let mut f = Fleet::<Fake>::default();
        f.apply(Some(&cfg(&["a"])), &LIVE).unwrap();
        log();
        fail(&["a"]);
        assert!(f.apply(None, &LIVE).is_err());
        fail(&[]);
        f.apply(Some(&cfg(&["a"])), &LIVE).unwrap();
        assert!(log().is_empty(), "a zeigt die Farbe schon");
        assert!(f.leaving.is_empty());
        f.restore().unwrap();
        assert_eq!(log(), ["a zurück"]);
    }

    #[test]
    fn duplicate_addresses_count_once() {
        let mut f = Fleet::<Fake>::default();
        f.apply(Some(&cfg(&["a", "a"])), &LIVE).unwrap();
        assert_eq!(log(), ["a Farbe"]);
        assert_eq!(f.devices.len(), 1);
    }
}
