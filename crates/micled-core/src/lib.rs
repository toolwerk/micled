//! MicLED-Kern: schaltet eine Luxafor-Lampe auf eine Mikrofarbe, solange ein
//! Programm das Mikrofon nutzt, und springt danach zur vorherigen Farbe zurück.

pub mod color;
pub mod engine;
pub mod lamp;
pub mod mic;
pub mod monitor;
pub mod shared;

pub use color::Rgb;
pub use engine::{AfterMic, Away, Config, DEFAULT_IGNORED};
pub use lamp::Transport;
pub use mic::MicUser;
pub use monitor::{Monitor, Status, StatusListener};
