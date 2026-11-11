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
use crate::reminder::{self, Event, Reminders};
use crate::stats::{self, CallLog, WeekSum};
use crate::display::Scene;
use crate::presence;
use crate::{awtrix, caldav, camera, dim, govee, hue, nanoleaf, nc_status, onair, output, pixoo, robobloq, shared, slack, wled};

const POLL: Duration = Duration::from_millis(500);
const RETRY: Duration = Duration::from_secs(2);
/// Wechsel bei der Termin-Vorwarnung (je Abfrage einer), gerade Anzahl
const FLASH_STEPS: u8 = 10;
/// Abstand der Abrufe aus Nextcloud
const NEXTCLOUD_EVERY: Duration = Duration::from_secs(120);
/// Wartezeit nach einer Änderung der Zugangsdaten (Tippen nicht abrufen)
const NEXTCLOUD_SETTLE: Duration = Duration::from_secs(3);
/// Höchstens so lange beim Beenden auf die Hue-Lampen warten (ein Raum wird
/// Lampe für Lampe zurückgestellt), ebenso auf Govee, Nanoleaf und WLED
const HUE_SHUTDOWN: Duration = Duration::from_secs(5);
/// Abstand der Suche nach dem Lichtstreifen (Anstecken erkennen)
const STRIP_EVERY: Duration = Duration::from_secs(5);

/// Govee-Lampen
type GoveeFleet = output::Fleet<govee::Session>;
/// Nanoleaf-Lampen
type NanoleafFleet = output::Fleet<nanoleaf::Session>;
/// WLED-Geräte
type WledFleet = output::Fleet<wled::Session>;

/// Ergebnis eines Abrufs aus Nextcloud, gekennzeichnet mit der Runde
type Fetched = Arc<Mutex<Option<(u64, Result<Vec<Event>, String>)>>>;

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
    /// Ende des Konzentrations-Timers (Unix-Sekunden)
    pub timer_until: Option<u64>,
    /// Fokus von macOS ist an
    pub focus: bool,
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
    Config(Box<Config>),
    Color(Rgb),
    Away(Away, bool),
    Upcoming(Vec<Event>),
    /// Timer bis zu dieser Zeit (Unix-Sekunden), `None` beendet ihn
    Timer(Option<u64>),
    Focus(bool),
    /// „Testen“ beim Lichtstreifen, Ergebnis an den Sender
    TestStrip(Sender<Result<robobloq::Info, String>>),
    Stop,
}

/// So lange wartet „Testen“ beim Lichtstreifen höchstens auf das Ergebnis
const STRIP_TEST_WAIT: Duration = Duration::from_secs(15);

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
            timer_until: None,
            focus: false,
            base,
            shown: base,
            error: None,
        }));
        let calls_path = CallLog::default_path();
        let calls = Arc::new(Mutex::new(calls_path.as_deref().map(CallLog::load).unwrap_or_default()));
        let (tx, rx) = channel::<Cmd>();
        let st = status.clone();
        let log = calls.clone();
        let slack_error = Arc::new(Mutex::new(None));
        let nc_status_error = Arc::new(Mutex::new(None));
        let pixoo_error = Arc::new(Mutex::new(None));
        let awtrix_error = Arc::new(Mutex::new(None));
        let hue_error = Arc::new(Mutex::new(None));
        let govee_error = Arc::new(Mutex::new(None));
        let nanoleaf_error = Arc::new(Mutex::new(None));
        let wled_error = Arc::new(Mutex::new(None));
        let robobloq_error = Arc::new(Mutex::new(None));
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
                    flash_color: Rgb::OFF,
                    live: false,
                    camera_reported: false,
                    focused: false,
                    calls: log,
                    calls_path,
                    on_air_error: Arc::new(Mutex::new(None)),
                    local_events: vec![],
                    remote_events: vec![],
                    next_fetch: Some(Instant::now()),
                    round: 0,
                    fetched: Arc::new(Mutex::new(None)),
                    fetching: false,
                    calendar_error: None,
                    brightness: 100,
                    slack: presence::Updater::start("slack", slack_error.clone()),
                    slack_error,
                    nc_status: presence::Updater::start("nc-status", nc_status_error.clone()),
                    nc_status_error,
                    pixoo: output::Updater::start("pixoo", pixoo_error.clone()),
                    pixoo_error,
                    last_scene: None,
                    awtrix: output::Updater::start("awtrix", awtrix_error.clone()),
                    awtrix_error,
                    last_awtrix: None,
                    hue: output::Updater::start("hue", hue_error.clone()),
                    hue_error,
                    last_hue: None,
                    govee: output::Updater::start("govee", govee_error.clone()),
                    govee_error,
                    last_govee: None,
                    nanoleaf: output::Updater::start("nanoleaf", nanoleaf_error.clone()),
                    nanoleaf_error,
                    last_nanoleaf: None,
                    wled: output::Updater::start("wled", wled_error.clone()),
                    wled_error,
                    last_wled: None,
                    robobloq: output::Updater::start("robobloq", robobloq_error.clone()),
                    robobloq_error,
                    last_robobloq: None,
                    watch: robobloq::Watch::default(),
                    next_watch: Instant::now(),
                };
                // ein nach einem Absturz hängengebliebener Call-Status wird aufgeräumt
                w.report_presence();
                w.file_mtime = w.mtime();
                loop {
                    match rx.recv_timeout(POLL) {
                        Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Ok(Cmd::Config(c)) => w.set_config(*c),
                        Ok(Cmd::Color(c)) => w.user_color(c),
                        Ok(Cmd::Away(r, on)) => w.set_away(r, on),
                        Ok(Cmd::Timer(t)) => w.set_timer(t),
                        Ok(Cmd::Focus(on)) => {
                            if let Some(c) = w.engine.set_focus(on) {
                                w.push(c);
                            }
                        }
                        Ok(Cmd::TestStrip(reply)) => w.robobloq.run(move |session, cfg| {
                            let _ = reply.send(session.test(cfg));
                        }),
                        Ok(Cmd::Upcoming(events)) => {
                            w.local_events = events;
                            w.update_reminders();
                        }
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
                w.set_focused(false);
                w.set_live(false);
                // Pixoo, AWTRIX, Hue, Govee, Nanoleaf, WLED und Lichtstreifen gleichzeitig zurückstellen, nicht nacheinander
                let started = Instant::now();
                let pixoo = w.pixoo.begin_shutdown();
                let awtrix = w.awtrix.begin_shutdown();
                let hue = w.hue.begin_shutdown();
                let govee = w.govee.begin_shutdown();
                let nanoleaf = w.nanoleaf.begin_shutdown();
                let wled = w.wled.begin_shutdown();
                let strip = w.robobloq.begin_shutdown();
                for (done, limit) in [
                    (pixoo, Duration::from_secs(3)),
                    (awtrix, Duration::from_secs(3)),
                    (hue, HUE_SHUTDOWN),
                    (govee, HUE_SHUTDOWN),
                    (nanoleaf, HUE_SHUTDOWN),
                    (wled, HUE_SHUTDOWN),
                    // ein laufender „Testen“ (bis gut 5 s) kommt vor dem Zurückstellen dran
                    (strip, Duration::from_secs(3) + robobloq::TEST_RED + Duration::from_secs(4)),
                ] {
                    if let Some(done) = done {
                        let _ = done.recv_timeout(limit.saturating_sub(started.elapsed()));
                    }
                }
                // Slack und Nextcloud gleichzeitig, zusammen höchstens 3 s
                let started = Instant::now();
                for done in [w.slack.begin_shutdown(), w.nc_status.begin_shutdown()].into_iter().flatten() {
                    let _ = done.recv_timeout(Duration::from_secs(3).saturating_sub(started.elapsed()));
                }
            })
            .expect("Monitor-Thread");
        Monitor { tx, status, calls, thread: Mutex::new(Some(thread)) }
    }

    pub fn update_config(&self, cfg: Config) {
        let _ = self.tx.send(Cmd::Config(Box::new(cfg)));
    }

    /// Farbe direkt setzen (wird zur neuen ruhenden Farbe)
    pub fn set_color(&self, c: Rgb) {
        let _ = self.tx.send(Cmd::Color(c));
    }

    /// Ruhezustand, Bildschirm aus oder Sperre beginnt (`on = true`) bzw. endet
    pub fn set_away(&self, reason: Away, on: bool) {
        let _ = self.tx.send(Cmd::Away(reason, on));
    }

    /// Termine aus den Kalendern von macOS, laufende und kommende, für
    /// Vorwarnung und Endzeit
    pub fn set_events(&self, events: Vec<Event>) {
        let _ = self.tx.send(Cmd::Upcoming(events));
    }

    /// Konzentrations-Timer für `minutes` Minuten starten, 0 beendet ihn
    pub fn start_timer(&self, minutes: u32) {
        let until = (minutes > 0).then(|| unix_now() + minutes as u64 * 60);
        let _ = self.tx.send(Cmd::Timer(until));
    }

    /// Fokus von macOS beginnt oder endet
    pub fn set_focus(&self, on: bool) {
        let _ = self.tx.send(Cmd::Focus(on));
    }

    /// Lichtstreifen testen: Geräteinfo lesen und kurz rot leuchten, danach
    /// zeigt er wieder den Zustand von MicLED. Blockiert einige Sekunden.
    pub fn test_strip(&self) -> Result<robobloq::Info, String> {
        let (reply, result) = channel();
        let _ = self.tx.send(Cmd::TestStrip(reply));
        result.recv_timeout(STRIP_TEST_WAIT).unwrap_or_else(|_| Err("Lichtstreifen: Test ohne Antwort".into()))
    }

    /// Sekunden auf Sendung seit `since` (Unix-Sekunden), laufender Call eingeschlossen
    pub fn live_seconds_since(&self, since: u64) -> u64 {
        self.calls.lock().unwrap().total_since(since, unix_now())
    }

    /// Sekunden auf Sendung je Tag zwischen aufsteigenden Tagesgrenzen
    pub fn live_seconds_per_day(&self, bounds: &[u64]) -> Vec<u64> {
        self.calls.lock().unwrap().per_day(bounds, unix_now())
    }

    /// Gespeicherte Calls als CSV
    pub fn calls_csv(&self) -> String {
        self.calls.lock().unwrap().to_csv()
    }

    /// Summen der letzten `count` Kalenderwochen (laufende zuletzt) und die
    /// Vorwoche bis zur selben Zeit, in der Zeitzone des Macs
    pub fn weekly(&self, count: usize) -> (Vec<WeekSum>, WeekSum) {
        let now = unix_now();
        let calls = self.calls.lock().unwrap();
        (calls.weeks(count, now, stats::local_offset), calls.last_week_to_date(now, stats::local_offset))
    }

    /// Summen je Kalenderwoche als CSV
    pub fn weeks_csv(&self) -> String {
        self.calls.lock().unwrap().weeks_csv(unix_now(), stats::local_offset)
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
    /// Verbleibende Wechsel beim Blinken (Vorwarnung, Ende des Timers)
    flash_left: u8,
    flash_color: Rgb,
    /// Zuletzt nach außen gemeldeter Zustand „Auf Sendung“
    live: bool,
    camera_reported: bool,
    /// Zuletzt in der Statistik als Fokuszeit erfasst
    focused: bool,
    calls: Arc<Mutex<CallLog>>,
    calls_path: Option<PathBuf>,
    on_air_error: Arc<Mutex<Option<String>>>,
    /// Termine aus den Kalendern von macOS (von der App)
    local_events: Vec<Event>,
    /// Termine aus Nextcloud
    remote_events: Vec<Event>,
    /// Nächster Abruf aus Nextcloud (`None` = aus)
    next_fetch: Option<Instant>,
    /// Zählt Änderungen der Zugangsdaten; ältere Ergebnisse werden verworfen
    round: u64,
    fetched: Fetched,
    fetching: bool,
    calendar_error: Option<String>,
    /// Zuletzt angewandte Helligkeit in Prozent
    brightness: u8,
    slack: presence::Updater<slack::State>,
    slack_error: Arc<Mutex<Option<String>>>,
    nc_status: presence::Updater<nc_status::State>,
    nc_status_error: Arc<Mutex<Option<String>>>,
    pixoo: output::Updater<pixoo::Session>,
    pixoo_error: Arc<Mutex<Option<String>>>,
    /// Zuletzt an das Display gegebene Adresse und Szene
    last_scene: Option<(Option<String>, Scene)>,
    awtrix: output::Updater<awtrix::Session>,
    awtrix_error: Arc<Mutex<Option<String>>>,
    /// Zuletzt an die AWTRIX-Uhr gegebene Adresse und Szene
    last_awtrix: Option<(Option<String>, Scene)>,
    hue: output::Updater<hue::Session>,
    hue_error: Arc<Mutex<Option<String>>>,
    /// Zuletzt an die Hue-Lampe gegebenes Ziel und Szene
    last_hue: Option<(Option<hue::Hue>, Scene)>,
    govee: output::Updater<GoveeFleet>,
    govee_error: Arc<Mutex<Option<String>>>,
    /// Zuletzt an die Govee-Lampen gegebene Liste und Szene
    last_govee: Option<(Option<Vec<govee::Govee>>, Scene)>,
    nanoleaf: output::Updater<NanoleafFleet>,
    nanoleaf_error: Arc<Mutex<Option<String>>>,
    /// Zuletzt an die Nanoleaf-Lampen gegebene Liste und Szene
    last_nanoleaf: Option<(Option<Vec<nanoleaf::Nanoleaf>>, Scene)>,
    wled: output::Updater<WledFleet>,
    wled_error: Arc<Mutex<Option<String>>>,
    /// Zuletzt an die WLED-Geräte gegebene Liste und Szene
    last_wled: Option<(Option<Vec<wled::Wled>>, Scene)>,
    robobloq: output::Updater<robobloq::Session>,
    robobloq_error: Arc<Mutex<Option<String>>>,
    /// Zuletzt an den Lichtstreifen gegebenes Ziel und Szene
    last_robobloq: Option<(Option<robobloq::Strip>, Scene)>,
    /// Ist der Streifen angesteckt? Zählt das Wiederanstecken.
    watch: robobloq::Watch,
    next_watch: Instant,
}

/// Bild je Zusatzgerät nach dessen Auswahl der Zustände, gedimmt
#[derive(Debug, PartialEq)]
struct Scenes {
    pixoo: Scene,
    awtrix: Scene,
    hue: Scene,
    govee: Scene,
    nanoleaf: Scene,
    wled: Scene,
    robobloq: Scene,
}

impl Scenes {
    fn new(engine: &Engine, now: u64, brightness: u8) -> Self {
        let cfg = engine.config();
        let scene = |states| engine.scene_for(now, states).map_color(|c| dim::apply(c, brightness));
        Scenes {
            pixoo: scene(cfg.pixoo_states),
            awtrix: scene(cfg.awtrix_states),
            hue: scene(cfg.hue_states),
            govee: scene(cfg.govee_states),
            nanoleaf: scene(cfg.nanoleaf_states),
            wled: scene(cfg.wled_states),
            robobloq: scene(cfg.robobloq_states),
        }
    }
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
        let slack_changed = cfg.slack != self.engine.config().slack;
        let nc_status_changed = cfg.nextcloud_status != self.engine.config().nextcloud_status;
        if cfg.pixoo != self.engine.config().pixoo {
            *self.pixoo_error.lock().unwrap() = None;
        }
        if cfg.awtrix != self.engine.config().awtrix {
            *self.awtrix_error.lock().unwrap() = None;
        }
        if cfg.hue != self.engine.config().hue {
            *self.hue_error.lock().unwrap() = None;
        }
        if cfg.govee != self.engine.config().govee {
            *self.govee_error.lock().unwrap() = None;
        }
        if cfg.nanoleaf != self.engine.config().nanoleaf {
            *self.nanoleaf_error.lock().unwrap() = None;
        }
        if cfg.wled != self.engine.config().wled {
            *self.wled_error.lock().unwrap() = None;
        }
        if cfg.robobloq != self.engine.config().robobloq {
            *self.robobloq_error.lock().unwrap() = None;
            self.watch = robobloq::Watch { paths: None, connection: self.watch.connection };
            self.next_watch = Instant::now();
        }
        if cfg.nextcloud != self.engine.config().nextcloud {
            self.round += 1;
            self.fetching = false;
            self.calendar_error = None;
            self.remote_events.clear();
            self.next_fetch = cfg.nextcloud.as_ref().map(|_| Instant::now() + NEXTCLOUD_SETTLE);
            self.update_reminders();
        }
        if let Some(c) = self.engine.set_config(cfg) {
            self.push(c);
        }
        if url_changed {
            *self.on_air_error.lock().unwrap() = None;
        }
        if slack_changed {
            *self.slack_error.lock().unwrap() = None;
        }
        if nc_status_changed {
            *self.nc_status_error.lock().unwrap() = None;
        }
        if slack_changed || nc_status_changed {
            self.report_presence();
        }
    }

    fn update_reminders(&mut self) {
        let merged = reminder::merge(&self.local_events, &self.remote_events);
        self.reminders.set(reminder::starts(&merged));
        self.engine.set_events(merged);
    }

    /// Termine aus Nextcloud abrufen (eigener Thread) und Ergebnisse übernehmen
    fn poll_nextcloud(&mut self) {
        let done = self.fetched.lock().unwrap().take();
        if let Some((round, result)) = done {
            if round == self.round {
                self.fetching = false;
                match result {
                    Ok(events) => {
                        self.remote_events = events;
                        self.calendar_error = None;
                    }
                    Err(e) => {
                        log::warn!("{e}");
                        self.calendar_error = Some(e);
                    }
                }
                self.update_reminders();
            }
        }
        let Some(nc) = self.engine.config().nextcloud.clone() else { return };
        if self.fetching || self.next_fetch.is_some_and(|t| Instant::now() < t) {
            return;
        }
        self.fetching = true;
        self.next_fetch = Some(Instant::now() + NEXTCLOUD_EVERY);
        let (round, slot) = (self.round, self.fetched.clone());
        std::thread::spawn(move || {
            let r = caldav::fetch(&nc, unix_now());
            *slot.lock().unwrap() = Some((round, r));
        });
    }

    fn set_timer(&mut self, until: Option<u64>) {
        let c = match until {
            Some(t) => self.engine.start_timer(t),
            None => self.engine.stop_timer(),
        };
        if let Some(c) = c {
            self.push(c);
        }
    }

    /// Blinken starten (nur in Ruhe und per USB, der Webhook ist zu träge)
    fn flash(&mut self, color: Rgb) {
        if !self.engine.overriding() && !self.engine.dark() && self.engine.config().transport == Transport::Usb {
            self.flash_color = color;
            self.flash_left = FLASH_STEPS;
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
                    // das Stream Deck setzt die volle Helligkeit, beim Dimmen nachziehen
                    if self.brightness < 100 {
                        self.push(s.color);
                    }
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

        // 3. Ende des Timers und Termin-Vorwarnung
        let now = unix_now();
        if let Some(until) = self.engine.timer().filter(|&t| now >= t) {
            if let Some(c) = self.engine.stop_timer() {
                self.push(c);
            }
            // nur blinken, wenn das Ende gerade erst war (nicht nach dem Aufwachen)
            if now - until < 5 {
                self.flash(self.engine.config().timer_color);
            }
        }
        self.poll_nextcloud();
        let lead = self.engine.config().reminder_lead.as_secs();
        if self.engine.config().reminders && self.reminders.due(now, lead) && self.pending.is_none() {
            self.flash(self.engine.config().reminder_color);
        }

        // 4. Helligkeit: an den Grenzen der Dimm-Zeit die Farbe neu senden
        let brightness = dim::percent(self.engine.config().dim, dim::local_minute());
        if brightness != self.brightness {
            self.brightness = brightness;
            if let (None, Some(c)) = (self.pending, self.shown) {
                self.push(c);
            }
        }

        // 5. Lampe schalten (mit Wiederholung, falls sie gerade fehlt)
        if let Some(c) = self.pending {
            let due = self.last_try.map(|t| t.elapsed() >= RETRY).unwrap_or(true);
            if due {
                self.last_try = Some(Instant::now());
                match self.lamp.set(dim::apply(c, self.brightness)) {
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
        } else if self.flash_left > 0 && (self.engine.overriding() || self.engine.dark()) {
            self.flash_left = 0;
        } else if self.flash_left > 0 {
            self.flash_left -= 1;
            let c = if self.flash_left % 2 == 1 {
                self.flash_color
            } else {
                self.shown.unwrap_or(Rgb::OFF)
            };
            if let Err(e) = self.lamp.set(dim::apply(c, self.brightness)) {
                log::warn!("Blinken: {e}");
                self.flash_left = 0;
            }
        }

        // 6. Auf Sendung: Statistik und Meldung nach außen
        let live = self.engine.live();
        if live != self.live || (live && self.engine.camera_on() != self.camera_reported) {
            self.set_live(live);
        }

        // 7. Lichtstreifen: alle fünf Sekunden nachsehen, ob er (wieder) angesteckt ist
        if self.engine.config().robobloq && Instant::now() >= self.next_watch {
            self.next_watch = Instant::now() + STRIP_EVERY;
            match robobloq::find() {
                Ok(paths) => self.watch.seen(paths),
                Err(e) => {
                    self.watch.seen(Vec::new());
                    *self.robobloq_error.lock().unwrap() = Some(e);
                }
            }
        }

        // 8. Display, AWTRIX, Hue, Govee, Nanoleaf, WLED und Lichtstreifen: nur bei einer Änderung (oder neuem Ziel) senden
        let scenes = Scenes::new(&self.engine, now, self.brightness);
        let cfg = self.engine.config();
        let wanted = (cfg.hue.clone(), scenes.hue);
        if self.last_hue.as_ref() != Some(&wanted) {
            self.hue.send(wanted.0.clone(), wanted.1.clone());
            self.last_hue = Some(wanted);
        }
        let govee = &cfg.govee;
        let wanted = ((!govee.is_empty()).then(|| govee.clone()), scenes.govee);
        if self.last_govee.as_ref() != Some(&wanted) {
            self.govee.send(wanted.0.clone(), wanted.1.clone());
            self.last_govee = Some(wanted);
        }
        let leaves = &cfg.nanoleaf;
        let wanted = ((!leaves.is_empty()).then(|| leaves.clone()), scenes.nanoleaf);
        if self.last_nanoleaf.as_ref() != Some(&wanted) {
            self.nanoleaf.send(wanted.0.clone(), wanted.1.clone());
            self.last_nanoleaf = Some(wanted);
        }
        let wleds = &cfg.wled;
        let wanted = ((!wleds.is_empty()).then(|| wleds.clone()), scenes.wled);
        if self.last_wled.as_ref() != Some(&wanted) {
            self.wled.send(wanted.0.clone(), wanted.1.clone());
            self.last_wled = Some(wanted);
        }
        let strip = cfg.robobloq.then_some(robobloq::Strip { connection: self.watch.connection });
        let wanted = (strip, scenes.robobloq);
        if self.last_robobloq.as_ref() != Some(&wanted) {
            self.robobloq.send(wanted.0.clone(), wanted.1.clone());
            self.last_robobloq = Some(wanted);
        }
        // nur Pixoo und AWTRIX zeigen das Ende des Termins
        let until = self.engine.meeting_end(now).map(dim::local_minute_at);
        let wanted = (cfg.awtrix.clone(), scenes.awtrix.with_until(until));
        if self.last_awtrix.as_ref() != Some(&wanted) {
            self.awtrix.send(wanted.0.clone(), wanted.1.clone());
            self.last_awtrix = Some(wanted);
        }
        let wanted = (cfg.pixoo.clone(), scenes.pixoo.with_until(until));
        if self.last_scene.as_ref() != Some(&wanted) {
            self.pixoo.send(wanted.0.clone(), wanted.1.clone());
            self.last_scene = Some(wanted);
        }

        // 9. Fokuszeit für die Statistik
        self.set_focused(self.engine.focus_time());
    }

    /// Beginn und Ende der Fokuszeit in der Statistik festhalten
    fn set_focused(&mut self, on: bool) {
        if on == self.focused {
            return;
        }
        self.focused = on;
        let mut calls = self.calls.lock().unwrap();
        if on {
            calls.focus_start(unix_now());
        } else {
            calls.focus_stop(unix_now());
            if let Some(p) = &self.calls_path {
                if let Err(e) = calls.save(p) {
                    log::warn!("Statistik nicht schreibbar: {e}");
                }
            }
        }
    }

    /// Aktuellen Zustand „Auf Sendung“ an Slack und Nextcloud melden (falls eingerichtet)
    fn report_presence(&self) {
        self.slack.send(self.engine.config().slack.clone(), self.live);
        self.nc_status.send(self.engine.config().nextcloud_status.clone(), self.live);
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
        let changed = live != self.live;
        self.live = live;
        self.camera_reported = live && self.engine.camera_on();
        if changed {
            self.report_presence();
        }

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
            timer_until: self.engine.timer(),
            focus: self.engine.focus(),
            base: self.engine.base(),
            shown: self.shown,
            error: self
                .lamp_error
                .clone()
                .or_else(|| self.mic_error.clone())
                .or_else(|| self.on_air_error.lock().unwrap().clone())
                .or_else(|| self.calendar_error.clone())
                .or_else(|| self.slack_error.lock().unwrap().clone())
                .or_else(|| self.nc_status_error.lock().unwrap().clone())
                .or_else(|| self.pixoo_error.lock().unwrap().clone())
                .or_else(|| self.hue_error.lock().unwrap().clone())
                .or_else(|| self.govee_error.lock().unwrap().clone())
                .or_else(|| self.nanoleaf_error.lock().unwrap().clone())
                .or_else(|| self.wled_error.lock().unwrap().clone())
                .or_else(|| self.robobloq_error.lock().unwrap().clone())
                .or_else(|| self.awtrix_error.lock().unwrap().clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::OutputStates;

    /// Name, Auswahl abschalten, Szene lesen
    type Device = (&'static str, fn(&mut Config), fn(&Scenes) -> &Scene);

    const NOTHING: OutputStates = OutputStates { call: false, timer: false, focus: false };

    /// Engine mit laufendem Call; `off` setzt bei einem Gerät alle Zustände ab
    fn in_call(off: impl FnOnce(&mut Config)) -> Engine {
        let mut cfg = Config::default();
        off(&mut cfg);
        let mut e = Engine::new(cfg, None);
        e.on_mic(vec![MicUser { pid: 1, bundle_id: "us.zoom.xos".into(), name: "zoom".into() }], Instant::now());
        e
    }

    #[test]
    fn scenes_follow_each_outputs_own_selection() {
        let live = Scene::Live { camera: false, color: Rgb::RED, until: None };
        let devices: [Device; 7] = [
            ("pixoo", |c| c.pixoo_states = NOTHING, |s| &s.pixoo),
            ("awtrix", |c| c.awtrix_states = NOTHING, |s| &s.awtrix),
            ("hue", |c| c.hue_states = NOTHING, |s| &s.hue),
            ("govee", |c| c.govee_states = NOTHING, |s| &s.govee),
            ("nanoleaf", |c| c.nanoleaf_states = NOTHING, |s| &s.nanoleaf),
            ("wled", |c| c.wled_states = NOTHING, |s| &s.wled),
            ("robobloq", |c| c.robobloq_states = NOTHING, |s| &s.robobloq),
        ];
        for (name, off, _) in devices {
            let scenes = Scenes::new(&in_call(off), 0, 100);
            for (other, _, get) in devices {
                let want = if other == name { &Scene::Idle } else { &live };
                assert_eq!(get(&scenes), want, "{other}, wenn bei {name} alles abgewählt ist");
            }
        }
    }

    #[test]
    fn scenes_are_dimmed() {
        let scenes = Scenes::new(&in_call(|_| {}), 0, 50);
        assert_eq!(scenes.hue, Scene::Live { camera: false, color: dim::apply(Rgb::RED, 50), until: None });
    }
}
