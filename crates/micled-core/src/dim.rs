//! Abends dimmen: Die Lampe hat keine Helligkeitssteuerung, deshalb werden die
//! Farbwerte vor dem Senden heruntergerechnet. Engine und Statusdatei sehen
//! immer die ungedimmte Farbe.

use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dim {
    /// Beginn in Minuten nach Mitternacht (Ortszeit)
    pub from_min: u16,
    /// Ende in Minuten nach Mitternacht; kleiner als `from_min` = über Mitternacht
    pub to_min: u16,
    /// Helligkeit in Prozent, 1–100
    pub percent: u8,
}

/// Helligkeit in Prozent zur Uhrzeit `minute` (Minuten nach Mitternacht)
pub fn percent(dim: Option<Dim>, minute: u16) -> u8 {
    let Some(d) = dim else { return 100 };
    let inside = if d.from_min <= d.to_min {
        minute >= d.from_min && minute < d.to_min
    } else {
        minute >= d.from_min || minute < d.to_min
    };
    if inside {
        d.percent.clamp(1, 100)
    } else {
        100
    }
}

/// Farbe auf `percent` Prozent dimmen. Leuchtende Anteile bleiben bei mindestens 1,
/// damit die Lampe nicht ausgeht.
pub fn apply(c: Rgb, percent: u8) -> Rgb {
    if percent >= 100 {
        return c;
    }
    let f = |v: u8| if v == 0 { 0 } else { ((v as u16 * percent as u16 + 50) / 100).max(1) as u8 };
    Rgb { r: f(c.r), g: f(c.g), b: f(c.b) }
}

/// Minuten nach Mitternacht in Ortszeit (mit Sommerzeit)
pub fn local_minute() -> u16 {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    local_minute_at(now as u64)
}

/// Minuten nach Mitternacht in Ortszeit zur Zeit `t` (Unix-Sekunden)
pub fn local_minute_at(t: u64) -> u16 {
    let t = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    (tm.tm_hour * 60 + tm.tm_min) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENING: Dim = Dim { from_min: 18 * 60, to_min: 7 * 60, percent: 40 };

    #[test]
    fn window_over_midnight() {
        assert_eq!(percent(Some(EVENING), 17 * 60 + 59), 100);
        assert_eq!(percent(Some(EVENING), 18 * 60), 40);
        assert_eq!(percent(Some(EVENING), 0), 40);
        assert_eq!(percent(Some(EVENING), 7 * 60 - 1), 40);
        assert_eq!(percent(Some(EVENING), 7 * 60), 100);
    }

    #[test]
    fn window_within_a_day() {
        let lunch = Dim { from_min: 12 * 60, to_min: 13 * 60, percent: 50 };
        assert_eq!(percent(Some(lunch), 12 * 60 + 30), 50);
        assert_eq!(percent(Some(lunch), 13 * 60), 100);
        assert_eq!(percent(None, 12 * 60), 100);
    }

    #[test]
    fn dims_and_keeps_lit_channels() {
        assert_eq!(apply(Rgb::RED, 40), Rgb { r: 102, g: 0, b: 0 });
        assert_eq!(apply(Rgb { r: 2, g: 0, b: 255 }, 10), Rgb { r: 1, g: 0, b: 26 });
        assert_eq!(apply(Rgb::OFF, 40), Rgb::OFF);
        assert_eq!(apply(Rgb::RED, 100), Rgb::RED);
    }

    #[test]
    fn local_minute_at_is_a_time_of_day() {
        // Ortszeit hängt vom Rechner ab: nur Bereich und Abstand prüfen
        let t = 1_793_887_200; // 5. 11. 2026, 14:00 UTC
        assert!(local_minute_at(t) < 24 * 60);
        assert_eq!((local_minute_at(t + 60) + 24 * 60 - local_minute_at(t)) % (24 * 60), 1);
    }
}
