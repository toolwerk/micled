//! MicLED-Kern: schaltet eine Luxafor-Lampe auf eine Mikrofarbe, solange ein
//! Programm das Mikrofon nutzt, und springt danach zur vorherigen Farbe zurück.

pub mod caldav;
pub mod camera;
pub mod color;
pub mod command;
pub mod dim;
pub mod display;
pub mod engine;
pub mod lamp;
pub mod mic;
pub mod monitor;
pub mod onair;
pub mod reminder;
pub mod shared;
pub mod slack;
pub mod stats;

pub use caldav::Nextcloud;
pub use color::Rgb;
pub use engine::{AfterMic, Away, Config, DEFAULT_IGNORED};
pub use lamp::Transport;
pub use mic::MicUser;
pub use monitor::{Monitor, Status, StatusListener};
