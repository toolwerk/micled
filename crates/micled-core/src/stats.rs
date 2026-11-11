//! Wie lange war heute „auf Sendung“, wie viel Fokuszeit hatte die Woche?
//! Speichert Zeiträume (Unix-Sekunden) in
//! `~/Library/Application Support/net.staude.micled/calls.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Ältere Einträge werden beim Speichern verworfen
const KEEP_SECS: u64 = 365 * 24 * 3600;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallLog {
    sessions: Vec<(u64, u64)>,
    /// Fokuszeit (Timer oder Fokus von macOS); fehlt in Dateien älterer Versionen
    #[serde(default)]
    focus: Vec<(u64, u64)>,
    #[serde(skip)]
    open: Option<u64>,
    #[serde(skip)]
    focus_open: Option<u64>,
}

/// Summen einer Kalenderwoche (ISO 8601, Beginn Montag 00:00 Ortszeit)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekSum {
    /// Montag 00:00 Ortszeit (Unix-Sekunden)
    pub start: u64,
    /// Jahr der ISO-Woche (kann vom Kalenderjahr des Montags abweichen)
    pub year: i32,
    pub week: u32,
    /// Sekunden auf Sendung
    pub live: u64,
    /// Calls, die in der Woche begonnen haben
    pub calls: u32,
    /// Sekunden Fokuszeit
    pub focus: u64,
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
        self.prune(now);
    }

    pub fn focus_start(&mut self, now: u64) {
        self.focus_open.get_or_insert(now);
    }

    pub fn focus_stop(&mut self, now: u64) {
        if let Some(s) = self.focus_open.take() {
            if now > s {
                self.focus.push((s, now));
            }
        }
        self.prune(now);
    }

    fn prune(&mut self, now: u64) {
        self.sessions.retain(|&(_, e)| e + KEEP_SECS > now);
        self.focus.retain(|&(_, e)| e + KEEP_SECS > now);
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

    /// Summen der letzten `count` Kalenderwochen, die laufende zuletzt.
    /// `offset(t)` ist der UTC-Versatz der Ortszeit zur Zeit `t` in Sekunden.
    /// Zeiträume über eine Wochengrenze werden geteilt, laufende zählen bis
    /// `now`; ein Call zählt in der Woche, in der er begonnen hat.
    pub fn weeks(&self, count: usize, now: u64, offset: impl Fn(u64) -> i64) -> Vec<WeekSum> {
        week_starts(now, count, &offset).windows(2).map(|w| self.week_sum(w[0], w[1], now, &offset)).collect()
    }

    /// Vorwoche bis zum selben Abstand vom Montag wie `now` in der laufenden
    /// Woche, zum Vergleich mit der noch nicht vollen laufenden Woche
    pub fn last_week_to_date(&self, now: u64, offset: impl Fn(u64) -> i64) -> WeekSum {
        let b = week_starts(now, 2, &offset);
        let to = b[0] + now.saturating_sub(b[1]);
        self.week_sum(b[0], to.min(b[1]), now, &offset)
    }

    /// Summen für `from..to`; `from` ist ein Montag 00:00 Ortszeit
    fn week_sum(&self, from: u64, to: u64, now: u64, offset: &impl Fn(u64) -> i64) -> WeekSum {
        let within = |list: &[(u64, u64)], open: Option<u64>| -> u64 {
            list.iter()
                .copied()
                .chain(open.map(|s| (s, now)))
                .map(|(s, e)| e.min(to).saturating_sub(s.max(from)))
                .sum()
        };
        let calls = self.sessions.iter().map(|&(s, _)| s).chain(self.open).filter(|s| (from..to).contains(s)).count();
        let (year, week) = iso_week(local_day(from, offset));
        WeekSum {
            start: from,
            year,
            week,
            live: within(&self.sessions, self.open),
            calls: calls as u32,
            focus: within(&self.focus, self.focus_open),
        }
    }

    /// Summen je Woche als CSV: `week,start,live_seconds,calls,focus_seconds`,
    /// von der Woche des ältesten Eintrags bis zur laufenden
    pub fn weeks_csv(&self, now: u64, offset: impl Fn(u64) -> i64) -> String {
        let oldest = self
            .sessions
            .iter()
            .chain(&self.focus)
            .map(|&(s, _)| s)
            .chain(self.open)
            .chain(self.focus_open)
            .min()
            .unwrap_or(now)
            .min(now);
        let monday = |t: u64| {
            let d = local_day(t, &offset);
            d - weekday(d)
        };
        let count = ((monday(now) - monday(oldest)) / 7 + 1) as usize;
        let mut out = String::from("week,start,live_seconds,calls,focus_seconds\n");
        for w in self.weeks(count, now, &offset) {
            let (y, m, d) = crate::caldav::civil_from_days(local_day(w.start, &offset));
            out.push_str(&format!(
                "{}-W{:02},{y:04}-{m:02}-{d:02},{},{},{}\n",
                w.year, w.week, w.live, w.calls, w.focus
            ));
        }
        out
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

/// Montag 00:00 Ortszeit der letzten `count` Kalenderwochen (laufende zuletzt)
/// und der nächste Montag, also `count + 1` aufsteigende Grenzen.
/// `offset(t)` ist der UTC-Versatz der Ortszeit zur Zeit `t` in Sekunden.
pub fn week_starts(now: u64, count: usize, offset: impl Fn(u64) -> i64) -> Vec<u64> {
    let today = local_day(now, &offset);
    let monday = today - weekday(today);
    (0..=count as i64)
        .map(|i| {
            // Lokale Mitternacht in UTC, zweistufig wegen der Sommerzeit
            let local = (monday + 7 * (i - count as i64 + 1)) * 86400;
            let t0 = local - offset(local.max(0) as u64);
            (local - offset(t0.max(0) as u64)).max(0) as u64
        })
        .collect()
}

/// UTC-Versatz der Ortszeit des Macs zur Zeit `t` in Sekunden, 0 bei einem Fehler
pub fn local_offset(t: u64) -> i64 {
    let t = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i64
}

/// Tage seit 1970-01-01 in Ortszeit
fn local_day(t: u64, offset: &impl Fn(u64) -> i64) -> i64 {
    (t as i64 + offset(t)).div_euclid(86400)
}

/// 0 = Montag; 1970-01-01 war ein Donnerstag
fn weekday(day: i64) -> i64 {
    (day + 3).rem_euclid(7)
}

/// ISO-Jahr und -Woche des Tages `day`: Die Woche gehört zum Jahr ihres Donnerstags
fn iso_week(day: i64) -> (i32, u32) {
    let thursday = day - weekday(day) + 3;
    let (y, _, _) = crate::caldav::civil_from_days(thursday);
    let jan1 = crate::caldav::days_from_civil(y, 1, 1);
    (y as i32, ((thursday - jan1) / 7 + 1) as u32)
}

/// `2026-11-05T14:00:00Z`
fn iso(t: u64) -> String {
    let b = crate::caldav::format_utc(t);
    format!("{}-{}-{}T{}:{}:{}Z", &b[0..4], &b[4..6], &b[6..8], &b[9..11], &b[11..13], &b[13..15])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC-Zeit als Unix-Sekunden
    fn utc(y: i64, m: i64, d: i64, h: i64, mi: i64) -> u64 {
        (crate::caldav::days_from_civil(y, m, d) * 86400 + h * 3600 + mi * 60) as u64
    }

    /// Berlin 2026: MESZ vom 29.03. bis zum 25.10. um 01:00 UTC, sonst MEZ
    fn berlin(t: u64) -> i64 {
        if (utc(2026, 3, 29, 1, 0)..utc(2026, 10, 25, 1, 0)).contains(&t) {
            7200
        } else {
            3600
        }
    }

    fn week(log: &CallLog, now: u64) -> WeekSum {
        *log.weeks(1, now, berlin).last().unwrap()
    }

    #[test]
    fn weeks_start_monday_midnight_local_time() {
        let now = utc(2026, 11, 11, 12, 0); // Mittwoch
        assert_eq!(
            week_starts(now, 2, berlin),
            vec![utc(2026, 11, 1, 23, 0), utc(2026, 11, 8, 23, 0), utc(2026, 11, 15, 23, 0)]
        );
        assert_eq!(week_starts(utc(2026, 11, 8, 23, 0), 1, berlin)[0], utc(2026, 11, 8, 23, 0), "Montag 00:00");
        assert_eq!(week_starts(utc(2026, 11, 8, 22, 59), 1, berlin)[0], utc(2026, 11, 1, 23, 0), "Sonntag 23:59");
        assert_eq!(week_starts(now, 1, |_| 0), vec![utc(2026, 11, 9, 0, 0), utc(2026, 11, 16, 0, 0)]);
    }

    #[test]
    fn week_across_daylight_saving_change_has_169_hours() {
        let b = week_starts(utc(2026, 10, 28, 12, 0), 2, berlin);
        assert_eq!(b, vec![utc(2026, 10, 18, 22, 0), utc(2026, 10, 25, 23, 0), utc(2026, 11, 1, 23, 0)]);
        assert_eq!(b[1] - b[0], 169 * 3600);
    }

    #[test]
    fn call_across_sunday_midnight_is_split_and_counted_at_start() {
        let mut log = CallLog::default();
        log.start(utc(2026, 11, 8, 22, 30)); // Sonntag 23:30 Ortszeit
        log.stop(utc(2026, 11, 8, 23, 30)); // Montag 00:30
        let w = log.weeks(2, utc(2026, 11, 11, 12, 0), berlin);
        assert_eq!((w[0].live, w[0].calls), (1800, 1));
        assert_eq!((w[1].live, w[1].calls), (1800, 0));
    }

    #[test]
    fn focus_across_week_bound_is_split() {
        let mut log = CallLog::default();
        log.focus_start(utc(2026, 11, 8, 22, 0));
        log.focus_stop(utc(2026, 11, 9, 0, 0));
        let w = log.weeks(2, utc(2026, 11, 11, 12, 0), berlin);
        assert_eq!((w[0].focus, w[1].focus), (3600, 3600));
        assert_eq!(w[1].calls, 0);
    }

    #[test]
    fn running_call_and_focus_count_until_now() {
        let mut log = CallLog::default();
        let now = utc(2026, 11, 11, 12, 0);
        log.start(now - 600);
        log.focus_start(now - 900);
        let w = week(&log, now);
        assert_eq!((w.live, w.calls, w.focus), (600, 1, 900));
    }

    #[test]
    fn iso_week_at_year_change() {
        let log = CallLog::default();
        let w = week(&log, utc(2025, 12, 31, 12, 0));
        assert_eq!((w.year, w.week, w.start), (2026, 1, utc(2025, 12, 28, 23, 0)));
        let w = week(&log, utc(2027, 1, 1, 12, 0));
        assert_eq!((w.year, w.week, w.start), (2026, 53, utc(2026, 12, 27, 23, 0)));
        assert_eq!(week(&log, utc(2026, 11, 11, 12, 0)).week, 46);
    }

    #[test]
    fn last_week_to_date_stops_at_same_time_of_week() {
        let mut log = CallLog::default();
        let now = utc(2026, 11, 11, 12, 0); // Mittwoch 13:00
        log.start(utc(2026, 11, 4, 8, 0)); // Mittwoch der Vorwoche, davor
        log.stop(utc(2026, 11, 4, 9, 0));
        log.start(utc(2026, 11, 4, 11, 30)); // über die Vergleichsgrenze
        log.stop(utc(2026, 11, 4, 12, 30));
        log.start(utc(2026, 11, 5, 8, 0)); // Donnerstag der Vorwoche, danach
        log.stop(utc(2026, 11, 5, 9, 0));
        log.focus_start(utc(2026, 11, 3, 8, 0));
        log.focus_stop(utc(2026, 11, 3, 10, 0));
        let w = log.last_week_to_date(now, berlin);
        assert_eq!((w.year, w.week, w.start), (2026, 45, utc(2026, 11, 1, 23, 0)));
        assert_eq!((w.live, w.calls, w.focus), (3600 + 1800, 2, 7200));
        assert_eq!(log.weeks(2, now, berlin)[0].live, 3 * 3600, "ganze Vorwoche");
    }

    #[test]
    fn weeks_csv_from_oldest_entry() {
        let mut log = CallLog::default();
        let now = utc(2026, 11, 11, 12, 0);
        log.focus_start(utc(2026, 11, 3, 8, 0));
        log.focus_stop(utc(2026, 11, 3, 9, 0));
        log.start(utc(2026, 11, 10, 8, 0));
        log.stop(utc(2026, 11, 10, 8, 10));
        assert_eq!(
            log.weeks_csv(now, berlin),
            "week,start,live_seconds,calls,focus_seconds\n2026-W45,2026-11-02,0,0,3600\n2026-W46,2026-11-09,600,1,0\n"
        );
        assert_eq!(
            CallLog::default().weeks_csv(now, berlin),
            "week,start,live_seconds,calls,focus_seconds\n2026-W46,2026-11-09,0,0,0\n"
        );
    }

    #[test]
    fn old_file_without_focus_loads() {
        let log: CallLog = serde_json::from_str(r#"{"sessions":[[100,200]]}"#).unwrap();
        assert_eq!(log.total_since(0, 300), 100);
        assert!(log.focus.is_empty());
    }

    #[test]
    fn old_focus_is_dropped() {
        let mut l = CallLog::default();
        l.focus_start(100);
        l.focus_stop(200);
        l.start(KEEP_SECS + 300);
        l.stop(KEEP_SECS + 400);
        assert!(l.focus.is_empty());
    }

    #[test]
    fn local_offset_is_plausible() {
        // Plausibel, unabhängig von der Zeitzone der Testmaschine
        let o = local_offset(utc(2026, 11, 11, 12, 0));
        assert!((-14 * 3600..=14 * 3600).contains(&o));
    }

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
