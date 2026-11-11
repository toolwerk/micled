//! MicLED-Kern: schaltet eine Luxafor-Lampe auf eine Mikrofarbe, solange ein
//! Programm das Mikrofon nutzt, und springt danach zur vorherigen Farbe zurück.

pub mod awtrix;
pub mod caldav;
pub mod camera;
pub mod color;
pub mod command;
pub mod dim;
pub mod display;
pub mod engine;
pub mod govee;
pub mod hue;
pub mod hue_net;
pub mod lamp;
pub mod lang;
pub mod mic;
pub mod monitor;
pub mod mqtt;
pub mod nanoleaf;
pub mod nc_status;
pub mod onair;
pub mod output;
pub mod pixoo;
pub mod presence;
pub mod reminder;
pub mod robobloq;
pub mod shared;
pub mod slack;
pub mod stats;
pub mod wled;

pub use caldav::Nextcloud;
pub use color::Rgb;
pub use engine::{AfterMic, Away, Config, OutputStates, DEFAULT_IGNORED};
pub use lamp::Transport;
pub use mic::MicUser;
pub use monitor::{Monitor, Status, StatusListener};
