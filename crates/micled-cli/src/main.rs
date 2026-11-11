//! `micled` - Luxafor-Lampe und Mikrofon-Automatik ohne Oberfläche testen.

use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use micled_core::display::Scene;
use micled_core::output::Device;
use micled_core::{awtrix, caldav, camera, govee, hue, lamp, mic, nanoleaf, pixoo, robobloq, AfterMic, Config, Monitor, Rgb, Status, StatusListener, Transport};

#[derive(Parser)]
#[command(name = "micled", version, about = "Luxafor-Lampe nach Mikrofon-Status schalten")]
struct Cli {
    /// Webhook statt USB verwenden (Luxafor-ID aus der Luxafor-App, Tab „Webhook“)
    #[arg(long, global = true, value_name = "LUXAFOR_ID")]
    webhook: Option<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Feste Farbe setzen, z. B. `micled set ff8800`
    Set { color: String },
    /// Lampe ausschalten
    Off,
    /// Zeigt, welche Programme gerade das Mikrofon nutzen und ob eine Kamera läuft
    Mic,
    /// Termine der nächsten Stunde aus Nextcloud lesen.
    /// Das App-Passwort kommt aus der Umgebungsvariable MICLED_NEXTCLOUD_PASSWORD.
    Nextcloud {
        /// Adresse der Instanz, z. B. https://cloud.example.org
        server: String,
        user: String,
    },
    /// Pixoo 64 prüfen: zeigt fünf Sekunden „ON AIR“ und stellt dann den Kanal zurück
    Pixoo {
        /// Adresse im eigenen Netz, z. B. 192.168.1.50
        host: String,
    },
    /// AWTRIX-Uhr prüfen: zeigt fünf Sekunden „ON AIR“ und räumt es dann wieder ab
    Awtrix {
        /// Adresse im eigenen Netz, z. B. 192.168.1.60
        host: String,
    },
    /// Philips Hue über die Hue Bridge
    Hue {
        #[command(subcommand)]
        cmd: HueCmd,
    },
    /// Govee-Lampen mit „LAN Control“ im eigenen Netz
    Govee {
        #[command(subcommand)]
        cmd: GoveeCmd,
    },
    /// Nanoleaf-Lampen über die lokale OpenAPI
    Nanoleaf {
        #[command(subcommand)]
        cmd: NanoleafCmd,
    },
    /// Lichtstreifen mit Robobloq-Controller per USB (1A86:FE07)
    Strip {
        #[command(subcommand)]
        cmd: StripCmd,
    },
    /// Automatik laufen lassen, bis Strg-C
    Watch {
        /// Farbe bei aktivem Mikrofon
        #[arg(long, default_value = "ff0000")]
        on: String,
        /// Farbe nach dem Mikrofon (bei --fixed immer, sonst nur wenn keine vorherige Farbe bekannt ist)
        #[arg(long, default_value = "00ff00")]
        off: String,
        /// Nach dem Mikrofon immer auf --off statt auf die vorherige Farbe
        #[arg(long)]
        fixed: bool,
        /// Sekunden Mikro-Pause, bevor zurückgeschaltet wird
        #[arg(long, default_value_t = 3)]
        delay: u64,
        /// Kamera nicht auswerten
        #[arg(long)]
        no_camera: bool,
        /// Farbe bei aktiver Kamera
        #[arg(long, default_value = "ff00ff")]
        camera: String,
        /// Bei „Auf Sendung“ an/aus JSON an diese Adresse senden (POST)
        #[arg(long, value_name = "URL")]
        on_air_url: Option<String>,
        /// Weitere Programme ignorieren (Bundle-ID oder Prozessname), mehrfach möglich.
        /// Bekannte Audio-Dienste werden immer ignoriert.
        #[arg(long)]
        ignore: Vec<String>,
    },
}

#[derive(Subcommand)]
enum HueCmd {
    /// Koppeln: erst den Knopf an der Bridge drücken. Gibt App-Schlüssel und
    /// Fingerabdruck des Zertifikats aus.
    Pair {
        /// Adresse der Bridge im eigenen Netz, z. B. 192.168.1.20
        host: String,
    },
    /// Zeigt fünf Sekunden Rot und stellt den Zustand von vorher wieder her.
    /// Schlüssel und Fingerabdruck kommen aus MICLED_HUE_KEY und MICLED_HUE_FINGERPRINT.
    Test {
        host: String,
        /// ID der Lampe (oder mit --room des Raums) aus der CLIP-v2-API
        id: String,
        /// `id` ist ein Raum
        #[arg(long)]
        room: bool,
    },
}

#[derive(Subcommand)]
enum GoveeCmd {
    /// Geräte im eigenen Netz suchen
    Scan,
    /// Zeigt fünf Sekunden Rot und stellt den Zustand von vorher wieder her
    Test {
        /// Adresse im eigenen Netz, z. B. 192.168.1.31
        host: String,
    },
}

#[derive(Subcommand)]
enum NanoleafCmd {
    /// Geräte im eigenen Netz per Bonjour suchen
    Scan,
    /// Koppeln: erst den Einschaltknopf 5–7 Sekunden halten. Gibt den Schlüssel aus.
    Pair {
        /// Adresse im eigenen Netz, z. B. 192.168.1.40
        host: String,
    },
    /// Zeigt fünf Sekunden Rot und stellt den Zustand von vorher wieder her.
    /// Der Schlüssel kommt aus MICLED_NANOLEAF_KEY.
    Test { host: String },
}

#[derive(Subcommand)]
enum StripCmd {
    /// Zeigt Zahl der LEDs, Monitorgröße und Firmware
    Info,
    /// Zeigt fünf Sekunden Rot und schaltet den Streifen dann aus
    Test,
}

/// Fünf Sekunden Rot, dann zurückstellen (auch nach einem Fehler, die Farbe
/// kann trotzdem angekommen sein)
fn show_red<D: Device>(device: &mut D, cfg: &D::Cfg) -> Result<(), String> {
    let shown = device.apply(Some(cfg), &Scene::Live { camera: false, color: Rgb::RED, until: None });
    if shown.is_ok() {
        std::thread::sleep(Duration::from_secs(5));
    }
    let restored = device.restore();
    shown.and(restored)
}

fn print_found(list: Vec<(String, String)>) {
    if list.is_empty() {
        println!("Keine Geräte gefunden.");
    }
    for (host, name) in list {
        println!("{host:<22} {name}");
    }
}

fn govee_cmd(cmd: &GoveeCmd) -> Result<(), String> {
    match cmd {
        GoveeCmd::Scan => govee::discover().map(print_found),
        GoveeCmd::Test { host } => {
            let cfg = govee::Govee { host: govee::normalize(host).ok_or("Adresse fehlt")? };
            show_red(&mut govee::Session::default(), &cfg)?;
            println!("Govee-Lampe geschaltet und zurückgestellt");
            Ok(())
        }
    }
}

fn nanoleaf_cmd(cmd: &NanoleafCmd) -> Result<(), String> {
    let host = |h: &str| nanoleaf::normalize(h).ok_or("Adresse fehlt".to_string());
    match cmd {
        NanoleafCmd::Scan => nanoleaf::discover().map(print_found),
        NanoleafCmd::Pair { host: h } => {
            println!("Schlüssel: {}", nanoleaf::pair(&host(h)?)?);
            Ok(())
        }
        NanoleafCmd::Test { host: h } => {
            let cfg = nanoleaf::Nanoleaf {
                host: host(h)?,
                key: std::env::var("MICLED_NANOLEAF_KEY").map_err(|_| "MICLED_NANOLEAF_KEY fehlt")?,
            };
            show_red(&mut nanoleaf::Session::default(), &cfg)?;
            println!("Nanoleaf-Lampe geschaltet und zurückgestellt");
            Ok(())
        }
    }
}

fn strip_cmd(cmd: &StripCmd) -> Result<(), String> {
    match cmd {
        StripCmd::Info => {
            let info = robobloq::check()?;
            println!("LEDs:      {}\nMonitor:   {}\"\nFirmware:  {}", info.leds, info.inches, info.firmware_text());
            Ok(())
        }
        StripCmd::Test => {
            show_red(&mut robobloq::Session::default(), &robobloq::Strip { connection: 0 })?;
            println!("Lichtstreifen geschaltet und ausgeschaltet");
            Ok(())
        }
    }
}

fn hue_cmd(cmd: &HueCmd) -> Result<(), String> {
    match cmd {
        HueCmd::Pair { host } => {
            let h = pixoo::normalize(host).ok_or("Adresse der Bridge fehlt")?;
            let (key, fingerprint) = hue::pair(&h)?;
            println!("Schlüssel:      {key}\nFingerabdruck:  {fingerprint}");
            Ok(())
        }
        HueCmd::Test { host, id, room } => {
            let cfg = hue::Hue {
                host: pixoo::normalize(host).ok_or("Adresse der Bridge fehlt")?,
                key: std::env::var("MICLED_HUE_KEY").map_err(|_| "MICLED_HUE_KEY fehlt")?,
                fingerprint: std::env::var("MICLED_HUE_FINGERPRINT").map_err(|_| "MICLED_HUE_FINGERPRINT fehlt")?,
                target: hue::Target { kind: if *room { hue::Kind::Room } else { hue::Kind::Light }, id: id.clone() },
            };
            let mut session = hue::Session::default();
            let shown = session.apply(Some(&cfg), &Scene::Live { camera: false, color: Rgb::RED, until: None });
            if shown.is_ok() {
                std::thread::sleep(Duration::from_secs(5));
            }
            // auch nach einem Fehler zurückstellen, die Farbe kann trotzdem angekommen sein
            let restored = session.restore();
            shown.and(restored)?;
            println!("Hue-Lampe geschaltet und zurückgestellt");
            Ok(())
        }
    }
}

fn transport(cli: &Cli) -> Transport {
    match &cli.webhook {
        Some(id) => Transport::Webhook { luxafor_id: id.clone() },
        None => Transport::Usb,
    }
}

fn color(s: &str) -> Rgb {
    Rgb::parse(s).unwrap_or_else(|| {
        eprintln!("Ungültige Farbe „{s}“ - erwartet z. B. ff8800");
        std::process::exit(2)
    })
}

struct Printer;

impl StatusListener for Printer {
    fn status_changed(&self, s: Status) {
        let users: Vec<String> = s
            .mic_users
            .iter()
            .map(|u| if u.bundle_id.is_empty() { u.name.clone() } else { u.bundle_id.clone() })
            .collect();
        let shown = s.shown.map(|c| c.hex()).unwrap_or_else(|| "?".into());
        let base = s.base.map(|c| c.hex()).unwrap_or_else(|| "?".into());
        println!(
            "Mikro: {:<4} Kamera: {:<4} Lampe: {shown}  danach: {base}  {}{}",
            if users.is_empty() { "aus" } else { "an" },
            if s.camera { "an" } else { "aus" },
            if users.is_empty() { String::new() } else { format!("({}) ", users.join(", ")) },
            s.error.map(|e| format!("Fehler: {e}")).unwrap_or_default()
        );
    }
}

fn main() {
    let cli = Cli::parse();
    let t = transport(&cli);
    let result = match &cli.cmd {
        Cmd::Set { color: c } => lamp::open(&t).set(color(c)).map_err(|e| e.to_string()),
        Cmd::Off => lamp::open(&t).set(Rgb::OFF).map_err(|e| e.to_string()),
        Cmd::Mic => mic::users().map(|users| {
            if users.is_empty() {
                println!("Kein Programm nutzt gerade das Mikrofon.");
            }
            for u in users {
                println!("{:>6}  {:<40} {}", u.pid, u.bundle_id, u.name);
            }
            match camera::in_use() {
                Ok(on) => println!("Kamera: {}", if on { "an" } else { "aus" }),
                Err(e) => println!("Kamera: {e}"),
            }
        }),
        Cmd::Nextcloud { server, user } => {
            let nc = micled_core::Nextcloud {
                server: server.clone(),
                user: user.clone(),
                password: std::env::var("MICLED_NEXTCLOUD_PASSWORD").unwrap_or_default(),
            };
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            caldav::check(&nc).and_then(|n| {
                println!("{n} Kalender mit Terminen gefunden.");
                let events = caldav::fetch(&nc, now)?;
                if events.is_empty() {
                    println!("Keine Termine in der nächsten Stunde.");
                }
                for e in events {
                    let end = e.end.map(|t| format!(" bis {}", caldav::format_utc(t))).unwrap_or_default();
                    let when = if e.start <= now { "läuft".to_string() } else { format!("in {} min", (e.start - now) / 60) };
                    println!("{}{end}  {when}", caldav::format_utc(e.start));
                }
                Ok(())
            })
        }
        Cmd::Pixoo { host } => match pixoo::normalize(host) {
            None => Err("Adresse des Pixoo fehlt".to_string()),
            Some(h) => pixoo::check(&h).and_then(|()| {
                let mut session = pixoo::Session::default();
                let shown = session.apply(Some(&h), &Scene::Live { camera: false, color: Rgb::RED, until: None });
                if shown.is_ok() {
                    std::thread::sleep(Duration::from_secs(5));
                }
                // auch nach einem Fehler zurückstellen, das Bild kann trotzdem angekommen sein
                let restored = session.restore();
                shown.and(restored)?;
                println!("Pixoo erreichbar");
                Ok(())
            }),
        },
        Cmd::Awtrix { host } => match pixoo::normalize(host) {
            None => Err("Adresse der Uhr fehlt".to_string()),
            Some(h) => awtrix::check(&h).and_then(|()| {
                show_red(&mut awtrix::Session::default(), &h)?;
                println!("AWTRIX erreichbar");
                Ok(())
            }),
        },
        Cmd::Hue { cmd } => hue_cmd(cmd),
        Cmd::Govee { cmd } => govee_cmd(cmd),
        Cmd::Nanoleaf { cmd } => nanoleaf_cmd(cmd),
        Cmd::Strip { cmd } => strip_cmd(cmd),
        Cmd::Watch { on, off, fixed, delay, no_camera, camera, on_air_url, ignore } => {
            let cfg = Config {
                enabled: true,
                transport: t,
                mic_on_color: color(on),
                mic_off_color: color(off),
                after_mic: if *fixed { AfterMic::Fixed } else { AfterMic::Restore },
                release_delay: Duration::from_secs(*delay),
                ignored: Config::default().ignored.into_iter().chain(ignore.iter().cloned()).collect(),
                // Die CLI bekommt keine Meldungen zu Ruhezustand und Sperre
                off_when_away: false,
                camera: !no_camera,
                camera_color: color(camera),
                on_air_url: on_air_url.clone().unwrap_or_default(),
                ..Config::default()
            };
            println!("Automatik läuft, Strg-C beendet.");
            let _monitor = Monitor::start(cfg, None, Arc::new(Printer));
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
