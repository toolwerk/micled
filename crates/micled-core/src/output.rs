//! Gemeinsamer Hintergrund-Thread für Ausgaben im eigenen Netz (Pixoo 64,
//! AWTRIX, Philips Hue, Govee, Nanoleaf, WLED): Ein fehlendes Gerät hält die Lampe nicht
//! auf, Fehler landen im Fehler-Slot und werden nach einer Pause wiederholt.
//! [`Fleet`] steuert mehrere Geräte derselben Art als ein [`Device`].
//! [`bonjour`] sucht Geräte für Nanoleaf und WLED.

use std::marker::PhantomData;
use std::net::Ipv4Addr;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// Auftrag, der im Ausgabe-Thread mit dem Gerät und dem gewünschten Ziel läuft
type Job<D> = Box<dyn FnOnce(&mut D, Option<&<D as Device>::Cfg>) + Send>;

enum Msg<D: Device> {
    Update(Option<D::Cfg>, Scene),
    /// Zustand zurückstellen und bestätigen (beim Beenden der App)
    Shutdown(Sender<()>),
    /// Auftrag ausführen, danach den gewünschten Zustand wieder herstellen
    Run(Job<D>),
}

/// Wartezeit nach einer Änderung des Ziels (Tippen nicht senden)
const SETTLE: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_secs(30);

/// Schickt Szenen in einem eigenen Thread an ein [`Device`]. Ein Erfolg leert
/// den Fehler-Slot.
pub struct Updater<D: Device> {
    tx: Sender<Msg<D>>,
    _device: PhantomData<fn() -> D>,
}

impl<D: Device> Updater<D> {
    pub fn start(name: &str, error: Arc<Mutex<Option<String>>>) -> Updater<D> {
        let (tx, rx) = channel::<Msg<D>>();
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
                        Ok(Msg::Run(job)) => {
                            job(&mut device, wanted.0.as_ref());
                            dirty = true;
                            continue;
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
                            Ok(Msg::Run(job)) => job(&mut device, wanted.0.as_ref()),
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

    /// Auftrag im Ausgabe-Thread ausführen, zwischen zwei `apply`. Danach
    /// stellt `apply` den gewünschten Zustand sofort wieder her.
    pub fn run(&self, job: impl FnOnce(&mut D, Option<&D::Cfg>) + Send + 'static) {
        let _ = self.tx.send(Msg::Run(Box::new(job)));
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

/// So lange sammelt [`bonjour`] Antworten
const SCAN_WAIT: Duration = Duration::from_secs(3);

/// Name des Geräts aus dem Bonjour-Namen („Canvas 5B3C._nanoleafapi._tcp.local.“)
pub(crate) fn instance_name(fullname: &str, service: &str) -> String {
    fullname.strip_suffix(service).map(|s| s.trim_end_matches('.')).unwrap_or(fullname).replace("\\032", " ")
}

/// Geräte mit dem Bonjour-Dienst `service` (z. B. `_wled._tcp.local.`) im
/// eigenen Netz suchen. Blockiert etwa drei Sekunden, nicht aus einem
/// Ausgabe-Thread aufrufen: `mdns-sd` startet einen eigenen. `what` steht im
/// Fehlertext. Liefert IPv4-Adresse (die kleinste des Geräts), Port und Namen,
/// jedes Gerät einmal.
pub(crate) fn bonjour(service: &str, what: &str) -> Result<Vec<(Ipv4Addr, u16, String)>, String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let daemon = ServiceDaemon::new().map_err(|e| format!("{what}-Suche nicht möglich: {e}"))?;
    let rx = daemon.browse(service).map_err(|e| format!("{what}-Suche nicht möglich: {e}"))?;
    let deadline = Instant::now() + SCAN_WAIT;
    let mut found: Vec<(Ipv4Addr, u16, String)> = Vec::new();
    while let Ok(ev) = rx.recv_deadline(deadline) {
        if let ServiceEvent::ServiceResolved(info) = ev {
            let name = instance_name(info.get_fullname(), service);
            let mut ips: Vec<Ipv4Addr> = info.get_addresses_v4().into_iter().collect();
            ips.sort();
            let port = info.get_port();
            if let Some(&ip) = ips.first() {
                if !found.iter().any(|(i, p, _)| *i == ip && *p == port) {
                    found.push((ip, port, name));
                }
            }
        }
    }
    let _ = daemon.shutdown();
    Ok(found)
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
    /// Gewünschte Geräte (`Some`) zeigen `scene`, entfernte (`None`) werden
    /// zurückgestellt, alle gleichzeitig: Ein nicht erreichbares Gerät hält
    /// die anderen nicht auf. Liefert die Fehler aller Geräte, entfernte
    /// eingeschlossen, solange sie noch versucht werden.
    fn run(&mut self, wanted: &[Option<S::Cfg>], scene: &Scene) -> Vec<String> {
        let leaving: Vec<Result<(), String>>;
        let shown: Vec<Result<(), String>>;
        {
            let devices = self.devices.iter_mut().map(|(_, d)| d).zip(wanted.iter().cloned());
            let gone = self.leaving.iter_mut().map(|l| (&mut l.device, None));
            let jobs: Vec<(&mut S, Option<S::Cfg>)> = devices.chain(gone).collect();
            let results: Vec<Result<(), String>> = std::thread::scope(|s| {
                let handles: Vec<_> = jobs
                    .into_iter()
                    .map(|(d, cfg)| {
                        let scene = if cfg.is_some() { scene.clone() } else { Scene::Idle };
                        s.spawn(move || d.apply(cfg.as_ref(), &scene))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("Ausgabe abgestürzt".into()))).collect()
            });
            let mut results = results;
            leaving = results.split_off(wanted.len());
            shown = results;
        }
        let mut errors: Vec<String> = shown.into_iter().filter_map(Result::err).collect();
        self.settle_leaving(leaving, &mut errors);
        errors
    }

    /// Ergebnisse der entfernten Geräte auswerten: zurückgestellte fallen
    /// weg, die anderen werden bis zu fünfmal versucht; Fehler kommen nach `errors`
    fn settle_leaving(&mut self, results: Vec<Result<(), String>>, errors: &mut Vec<String>) {
        for (mut l, r) in std::mem::take(&mut self.leaving).into_iter().zip(results) {
            match r {
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

/// Fehler zu einer Meldung, gleiche nur einmal (z. B. belegter Port)
fn joined(mut errors: Vec<String>) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    errors.retain(|e| seen.insert(e.clone()));
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
        let mut targets: Vec<Option<S::Cfg>> = Vec::new();
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
            targets.push(Some(w.clone()));
        }
        self.current = cfg.cloned();
        let errors = self.run(&targets, scene);
        joined(errors)
    }

    fn restore(&mut self) -> Result<(), String> {
        let targets: Vec<Option<S::Cfg>> = self.devices.iter().map(|(_, d)| d.current().cloned()).collect();
        let errors = self.run(&targets, &Scene::Idle);
        joined(errors)
    }

    fn current(&self) -> Option<&Vec<S::Cfg>> {
        self.current.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    #[derive(Clone, Debug, PartialEq)]
    struct Cfg(&'static str);

    impl Keyed for Cfg {
        fn address(&self) -> &str {
            self.0
        }
    }

    /// Befehle aller Fake-Geräte (die Geräte laufen in eigenen Threads)
    static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
    /// Adressen, die gerade scheitern
    static FAILING: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    /// Die Tests teilen sich LOG und FAILING, deshalb nacheinander
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> MutexGuard<'static, ()> {
        let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        LOG.lock().unwrap().clear();
        FAILING.lock().unwrap().clear();
        g
    }

    /// Befehle in fester Reihenfolge (die Threads laufen gleichzeitig)
    fn log() -> Vec<String> {
        let mut l = std::mem::take(&mut *LOG.lock().unwrap());
        l.sort();
        l
    }

    fn fail(hosts: &[&'static str]) {
        *FAILING.lock().unwrap() = hosts.to_vec();
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
            if target.0.starts_with("langsam") {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            if FAILING.lock().unwrap().contains(&target.0) {
                return Err(format!("{} antwortet nicht", target.0));
            }
            let show = cfg.is_some() && *scene != Scene::Idle;
            if show != self.shown {
                LOG.lock().unwrap().push(format!("{} {}", target.0, if show { "Farbe" } else { "zurück" }));
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

    const LIVE: Scene = Scene::Live { camera: false, color: Rgb::RED, until: None };

    fn cfg(hosts: &[&'static str]) -> Vec<Cfg> {
        hosts.iter().map(|h| Cfg(h)).collect()
    }

    #[test]
    fn one_failing_device_does_not_stop_the_others() {
        let _s = serial();
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
        let _s = serial();
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
        let _s = serial();
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
        let _s = serial();
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
    fn unreachable_devices_do_not_delay_the_others() {
        let _s = serial();
        let mut f = Fleet::<Fake>::default();
        fail(&["langsam1", "langsam2"]);
        let start = std::time::Instant::now();
        assert!(f.apply(Some(&cfg(&["langsam1", "langsam2", "langsam3"])), &LIVE).is_err());
        assert!(start.elapsed() < std::time::Duration::from_millis(550), "gleichzeitig, nicht nacheinander");
        assert_eq!(log(), ["langsam3 Farbe"]);
    }

    #[test]
    fn same_error_is_shown_once() {
        assert_eq!(joined(vec!["Port belegt".into(), "x".into(), "Port belegt".into()]).unwrap_err(), "Port belegt; x");
        assert_eq!(joined(vec![]), Ok(()));
    }

    /// Gerät, das jedes `apply` über einen Kanal meldet
    #[derive(Default)]
    struct Probe {
        cfg: Option<Cfg>,
        tx: Option<Sender<String>>,
    }

    impl Device for Probe {
        type Cfg = Cfg;

        fn apply(&mut self, cfg: Option<&Cfg>, scene: &Scene) -> Result<(), String> {
            self.cfg = cfg.cloned();
            if let Some(tx) = &self.tx {
                let _ = tx.send(format!("apply {:?} {scene:?}", cfg.map(|c| c.0)));
            }
            Ok(())
        }

        fn restore(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn current(&self) -> Option<&Cfg> {
            self.cfg.as_ref()
        }
    }

    #[test]
    fn run_is_followed_by_the_wanted_state() {
        let u = Updater::<Probe>::start("probe", Arc::new(Mutex::new(None)));
        let (tx, rx) = channel();
        let t = tx.clone();
        u.run(move |d, cfg| {
            d.tx = Some(t.clone());
            let _ = t.send(format!("run {:?}", cfg.map(|c| c.0)));
        });
        let wait = Duration::from_secs(5);
        assert_eq!(rx.recv_timeout(wait).unwrap(), "run None");
        assert_eq!(rx.recv_timeout(wait).unwrap(), "apply None Idle", "auch ohne neues Ziel");
        u.send(Some(Cfg("a")), LIVE);
        assert!(rx.recv_timeout(wait).unwrap().starts_with("apply Some(\"a\") Live"));
        u.run(move |_, cfg| {
            let _ = tx.send(format!("run {:?}", cfg.map(|c| c.0)));
        });
        assert_eq!(rx.recv_timeout(wait).unwrap(), "run Some(\"a\")", "mit dem gewünschten Ziel");
        assert!(rx.recv_timeout(wait).unwrap().starts_with("apply Some(\"a\") Live"), "Szene wieder hergestellt");
        u.shutdown(wait);
    }

    #[test]
    fn duplicate_addresses_count_once() {
        let _s = serial();
        let mut f = Fleet::<Fake>::default();
        f.apply(Some(&cfg(&["a", "a"])), &LIVE).unwrap();
        assert_eq!(log(), ["a Farbe"]);
        assert_eq!(f.devices.len(), 1);
    }
}
