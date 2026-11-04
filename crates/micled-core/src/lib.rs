//! MicLED-Kern: schaltet eine Luxafor-Lampe auf eine Mikrofarbe, solange ein
//! Programm das Mikrofon nutzt, und springt danach zur vorherigen Farbe zurück.

pub mod camera;
pub mod color;
pub mod command;
pub mod engine;
pub mod lamp;
pub mod mic;
pub mod monitor;
pub mod onair;
pub mod reminder;
pub mod shared;
pub mod stats;

pub use color::Rgb;
pub use engine::{AfterMic, Away, Config, Side, DEFAULT_IGNORED};
pub use lamp::{Frame, Transport};
pub use mic::MicUser;
pub use monitor::{Monitor, Status, StatusListener};
