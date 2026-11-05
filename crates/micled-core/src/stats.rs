//! Wie lange war heute „auf Sendung“? Speichert Zeiträume (Unix-Sekunden)
//! in `~/Library/Application Support/net.staude.micled/calls.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Ältere Einträge werden beim Speichern verworfen
const KEEP_SECS: u64 = 365 * 24 * 3600;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallLog {
    sessions: Vec<(u64, u64)>,
    #[serde(skip)]
    open: Option<u64>,
}

impl CallLog {
    pub fn start(&mut self, now: u64) {
        self.open.get_or_insert(now);
    }

    pub fn stop(&mut self, now: u64) {
        if let Some(s) = self.open.take() {
            if now > s {
                self.sessions.push((s, now));
            }
        }
        self.sessions.retain(|&(_, e)| e + KEEP_SECS > now);
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Sekunden auf Sendung seit `since`, einschließlich eines laufenden Calls
    pub fn total_since(&self, since: u64, now: u64) -> u64 {
        let closed = self.sessions.iter().map(|&(s, e)| e.saturating_sub(s.max(since)));
        let open = self.open.map(|s| now.saturating_sub(s.max(since)));
        closed.chain(open).sum()
    }

    /// Sekunden auf Sendung je Tag. `bounds` sind aufsteigende Tagesgrenzen
    /// (lokale Mitternachten), Ergebnis hat einen Eintrag weniger. Ein Call über
    /// Mitternacht wird aufgeteilt, ein laufender zählt bis `now`.
    pub fn per_day(&self, bounds: &[u64], now: u64) -> Vec<u64> {
        let open = self.open.map(|s| (s, now));
        bounds
            .windows(2)
            .map(|w| {
                let (from, to) = (w[0], w[1]);
                self.sessions
                    .iter()
                    .copied()
                    .chain(open)
                    .map(|(s, e)| e.min(to).saturating_sub(s.max(from)))
                    .sum()
            })
            .collect()
    }

    /// Alle gespeicherten Calls als CSV: `start,end,seconds` in UTC (ISO 8601)
    pub fn to_csv(&self) -> String {
        let mut out = String::from("start,end,seconds\n");
        for &(s, e) in &self.sessions {
            out.push_str(&format!("{},{},{}\n", iso(s), iso(e), e.saturating_sub(s)));
        }
        out
    }

    pub fn default_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/net.staude.micled/calls.json"))
    }

    /// Ein beim Beenden noch laufender Call wird nicht gespeichert.
    pub fn load(path: &Path) -> CallLog {
        fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(self)?)?;
        fs::rename(&tmp, path)
    }
}

/// `2026-11-05T14:00:00Z`
fn iso(t: u64) -> String {
    let b = crate::caldav::format_utc(t);
    format!("{}-{}-{}T{}:{}:{}Z", &b[0..4], &b[4..6], &b[6..8], &b[9..11], &b[11..13], &b[13..15])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_day_splits_at_bounds() {
        let mut log = CallLog::default();
        log.start(90);
        log.stop(130); // 10 s an Tag 1, 30 s an Tag 2
        log.start(150);
        log.stop(160);
        log.start(250); // läuft noch
        assert_eq!(log.per_day(&[0, 100, 200, 300], 260), vec![10, 40, 10]);
        assert_eq!(log.per_day(&[0], 260), Vec::<u64>::new());
    }

    #[test]
    fn csv_format() {
        let mut log = CallLog::default();
        log.start(1793887200);
        log.stop(1793887200 + 90);
        assert_eq!(log.to_csv(), "start,end,seconds\n2026-11-05T14:00:00Z,2026-11-05T14:01:30Z,90\n");
    }

    #[test]
    fn sums_closed_and_running_sessions_since() {
        let mut l = CallLog::default();
        l.start(100);
        l.stop(200);
        l.start(500);
        assert_eq!(l.total_since(0, 600), 200);
        assert_eq!(l.total_since(150, 600), 150, "Beginn vor `since` wird abgeschnitten");
        assert_eq!(l.total_since(700, 800), 100, "laufender Call zählt ab `since`");
    }

    #[test]
    fn start_twice_keeps_first_and_stop_without_start_is_ignored() {
        let mut l = CallLog::default();
        l.stop(50);
        l.start(100);
        l.start(150);
        l.stop(200);
        assert_eq!(l.total_since(0, 200), 100);
        assert!(!l.is_open());
    }

    #[test]
    fn old_sessions_are_dropped() {
        let mut l = CallLog::default();
        l.start(100);
        l.stop(200);
        l.start(KEEP_SECS + 300);
        l.stop(KEEP_SECS + 400);
        assert_eq!(l.total_since(0, KEEP_SECS + 400), 100);
    }

    #[test]
    fn save_and_load_drop_running_call() {
        let dir = std::env::temp_dir().join(format!("micled-stats-{}", std::process::id()));
        let path = dir.join("calls.json");
        let mut l = CallLog::default();
        l.start(100);
        l.stop(200);
        l.start(300);
        l.save(&path).unwrap();
        let back = CallLog::load(&path);
        assert_eq!(back.total_since(0, 1000), 100);
        assert!(!back.is_open());
        fs::remove_dir_all(dir).ok();
    }
}
