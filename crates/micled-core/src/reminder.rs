//! Vorwarnung vor Terminen: entscheidet, wann die Lampe kurz blinken soll.
//! Die Termine kommen aus den Kalendern von macOS und aus Nextcloud
//! (Zeiten in Unix-Sekunden).

use std::collections::HashSet;

/// Ein Termin aus einem Kalender, ohne Titel
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Event {
    pub start: u64,
    /// Ende, falls bekannt
    pub end: Option<u64>,
    pub all_day: bool,
    /// Abgelehnt oder abgesagt
    pub declined: bool,
}

impl Event {
    /// Zählt für Vorwarnung und Endzeit: nicht ganztägig, nicht abgelehnt
    pub fn counts(&self) -> bool {
        !self.all_day && !self.declined
    }
}

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

/// Termine aus mehreren Quellen zusammenführen. Ein Termin, der in beiden
/// steht (Nextcloud auch in macOS eingebunden), zählt einmal. Hat ihn eine
/// Quelle als abgelehnt oder ganztägig, gilt das für beide: Nextcloud kennt
/// abgelehnte Termine nicht.
pub fn merge(a: &[Event], b: &[Event]) -> Vec<Event> {
    let mut all: Vec<Event> = a.iter().chain(b).copied().collect();
    all.sort_unstable();
    all.dedup_by(|later, kept| {
        let same = (later.start, later.end) == (kept.start, kept.end);
        if same {
            kept.all_day |= later.all_day;
            kept.declined |= later.declined;
        }
        same
    });
    all
}

/// Startzeiten der zählenden Termine für die Vorwarnung, sortiert, ohne Doppelte
pub fn starts(events: &[Event]) -> Vec<u64> {
    let mut out: Vec<u64> = events.iter().filter(|e| e.counts()).map(|e| e.start).collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(start: u64, end: u64) -> Event {
        Event { start, end: Some(end), all_day: false, declined: false }
    }

    #[test]
    fn merge_drops_duplicates() {
        let merged = merge(&[ev(2000, 2600), ev(1000, 1600)], &[ev(1000, 1600), ev(3000, 3600)]);
        assert_eq!(merged, vec![ev(1000, 1600), ev(2000, 2600), ev(3000, 3600)]);
        assert_eq!(merge(&[], &[]), Vec::<Event>::new());
        let longer = merge(&[ev(1000, 1600)], &[ev(1000, 4000)]);
        assert_eq!(longer.len(), 2, "gleicher Start, anderes Ende: zwei Termine");
        let declined = Event { declined: true, ..ev(1000, 1600) };
        assert_eq!(merge(&[declined], &[ev(1000, 1600)]), vec![declined], "abgelehnt in einer Quelle");
    }

    #[test]
    fn starts_skip_all_day_and_declined() {
        let all_day = Event { all_day: true, ..ev(500, 90000) };
        let declined = Event { declined: true, ..ev(700, 900) };
        let events = [ev(2000, 2600), all_day, declined, ev(1000, 1600), ev(1000, 1900)];
        assert_eq!(starts(&events), vec![1000, 2000]);
    }

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
