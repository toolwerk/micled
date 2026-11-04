//! Vorwarnung vor Terminen: entscheidet, wann die Lampe kurz blinken soll.
//! Die Startzeiten liefert die App aus dem Kalender (Unix-Sekunden).

use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Reminders {
    starts: Vec<u64>,
    fired: HashSet<u64>,
}

impl Reminders {
    /// Startzeiten der nächsten Termine ersetzen
    pub fn set(&mut self, starts: Vec<u64>) {
        self.starts = starts;
    }

    /// `true`, wenn jetzt eine Vorwarnung fällig ist. Jeder Startzeitpunkt
    /// warnt nur einmal, auch wenn mehrere Termine gleichzeitig beginnen.
    pub fn due(&mut self, now: u64, lead: u64) -> bool {
        self.fired.retain(|&s| s + 3600 > now);
        let mut due = false;
        for &s in &self.starts {
            if now < s && now + lead >= s && self.fired.insert(s) {
                due = true;
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_once_inside_lead_time() {
        let mut r = Reminders::default();
        r.set(vec![1000]);
        assert!(!r.due(800, 120), "zu früh");
        assert!(r.due(900, 120));
        assert!(!r.due(930, 120), "nur einmal");
    }

    #[test]
    fn no_warning_once_started() {
        let mut r = Reminders::default();
        r.set(vec![1000]);
        assert!(!r.due(1000, 120));
        assert!(!r.due(1200, 120));
    }

    #[test]
    fn simultaneous_events_warn_once() {
        let mut r = Reminders::default();
        r.set(vec![1000, 1000]);
        assert!(r.due(950, 120));
        assert!(!r.due(951, 120));
    }

    #[test]
    fn refreshed_list_does_not_warn_again() {
        let mut r = Reminders::default();
        r.set(vec![1000]);
        assert!(r.due(900, 120));
        r.set(vec![1000, 2000]);
        assert!(!r.due(910, 120));
        assert!(r.due(1900, 120));
    }
}
