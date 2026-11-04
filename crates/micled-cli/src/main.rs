//! `micled` - Luxafor-Lampe und Mikrofon-Automatik ohne Oberfläche testen.

use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use micled_core::{camera, lamp, mic, AfterMic, Config, Frame, Monitor, Rgb, Status, StatusListener, Transport};

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
    /// Feste Farbe setzen, z. B. `micled set ff8800` oder nur eine Seite mit `--side a`
    Set {
        color: String,
        /// Nur Seite a (LEDs 1–3) oder b (LEDs 4–6) setzen, die andere geht aus
        #[arg(long, value_parser = ["a", "b"])]
        side: Option<String>,
    },
    /// Lampe ausschalten
    Off,
    /// Zeigt, welche Programme gerade das Mikrofon nutzen und ob eine Kamera läuft
    Mic,
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
        Cmd::Set { color: c, side } => {
            let c = color(c);
            let f = match side.as_deref() {
                Some("a") => Frame { a: c, b: Rgb::OFF },
                Some("b") => Frame { a: Rgb::OFF, b: c },
                _ => Frame::solid(c),
            };
            lamp::open(&t).set_frame(f).map_err(|e| e.to_string())
        }
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
