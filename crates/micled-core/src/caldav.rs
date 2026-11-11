//! Termine direkt aus einem Nextcloud-Kalender lesen (CalDAV), für die
//! Vorwarnung ohne Umweg über die Kalender von macOS.
//!
//! Ablauf je Abruf:
//! 1. `PROPFIND /remote.php/dav/` → `current-user-principal`
//! 2. `PROPFIND <principal>` → `calendar-home-set`
//! 3. `PROPFIND <home>` (Tiefe 1) → Kalender, die Termine (VEVENT) führen
//! 4. je Kalender `REPORT` mit `time-range` und `expand`: Der Server löst
//!    Serien auf und liefert die Zeiten in UTC.

use std::time::Duration;

use base64::Engine as _;

use crate::tr;

/// Zugang zu einer Nextcloud. `server` ist die Adresse der Instanz,
/// z. B. `https://cloud.example.org` oder `https://example.org/nextcloud`.
#[derive(Clone, PartialEq, Eq)]
pub struct Nextcloud {
    pub server: String,
    pub user: String,
    /// App-Passwort aus Nextcloud › Einstellungen › Sicherheit
    pub password: String,
}

impl std::fmt::Debug for Nextcloud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nextcloud").field("server", &self.server).field("user", &self.user).finish_non_exhaustive()
    }
}

/// So weit voraus werden Termine gelesen (wie bei den Kalendern von macOS)
pub const HORIZON: u64 = 3600;

const DAV: &str = "DAV:";
const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";

/// `https://` ergänzen, Schrägstrich am Ende entfernen
pub fn normalize(server: &str) -> String {
    let s = server.trim().trim_end_matches('/');
    if s.contains("://") {
        s.to_string()
    } else {
        format!("https://{s}")
    }
}

/// `https://host[:port]` einer Adresse, um absolute Pfade aus `href` aufzulösen
fn origin(url: &str) -> &str {
    let start = url.find("://").map(|i| i + 3).unwrap_or(0);
    match url[start..].find('/') {
        Some(i) => &url[..start + i],
        None => url,
    }
}

/// Das Passwort geht nur verschlüsselt raus; `http://` nur für den eigenen Rechner
pub fn check_secure(server: &str) -> Result<(), String> {
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"];
    let ok = server.starts_with("https://")
        || local.iter().any(|l| server == *l || server.starts_with(&format!("{l}:")) || server.starts_with(&format!("{l}/")));
    if ok {
        Ok(())
    } else {
        Err(tr!("Nextcloud nur über https:// erreichbar eintragen", "Enter a Nextcloud address with https://"))
    }
}

/// `href` aus einer Antwort zur vollen Adresse machen. Adressen auf anderen
/// Servern werden abgelehnt, damit die Zugangsdaten dort nicht hingehen.
fn resolve(base: &str, href: &str) -> Result<String, String> {
    if !href.contains("://") {
        return Ok(format!("{}{href}", origin(base)));
    }
    if origin(href) == origin(base) {
        Ok(href.to_string())
    } else {
        Err(tr!("Nextcloud verweist auf einen anderen Server: {}", "Nextcloud points to another server: {}", origin(href)))
    }
}

fn parse(xml: &str) -> Result<roxmltree::Document<'_>, String> {
    roxmltree::Document::parse(xml).map_err(|e| tr!("Antwort von Nextcloud unlesbar: {e}", "Unreadable reply from Nextcloud: {e}"))
}

/// Erstes `<href>` innerhalb des Elements `prop` (z. B. `current-user-principal`)
pub fn href_of(xml: &str, prop: &str) -> Result<Option<String>, String> {
    let doc = parse(xml)?;
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name((DAV, prop)) || n.has_tag_name((CALDAV, prop)))
        .flat_map(|n| n.descendants())
        .find(|n| n.has_tag_name((DAV, "href")))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string()))
}

/// Adressen (`href`) der Kalender, die Termine führen. Aufgaben-Listen,
/// Abos, Posteingang und Papierkorb fallen weg.
pub fn calendars(xml: &str) -> Result<Vec<String>, String> {
    let doc = parse(xml)?;
    let mut out = vec![];
    for r in doc.descendants().filter(|n| n.has_tag_name((DAV, "response"))) {
        let is_calendar = r.descendants().any(|n| n.has_tag_name((CALDAV, "calendar")));
        let comps: Vec<&str> = r
            .descendants()
            .filter(|n| n.has_tag_name((CALDAV, "comp")))
            .filter_map(|n| n.attribute("name"))
            .collect();
        // ohne Angabe der Komponenten gelten alle (RFC 4791, 5.2.3)
        let events = comps.is_empty() || comps.contains(&"VEVENT");
        let href = r.children().find(|n| n.has_tag_name((DAV, "href"))).and_then(|n| n.text());
        if let (true, true, Some(h)) = (is_calendar, events, href) {
            out.push(h.trim().to_string());
        }
    }
    Ok(out)
}

/// Startzeiten (Unix-Sekunden) aus einer `REPORT`-Antwort
pub fn starts(xml: &str) -> Result<Vec<u64>, String> {
    let doc = parse(xml)?;
    let mut out = vec![];
    for data in doc.descendants().filter(|n| n.has_tag_name((CALDAV, "calendar-data"))) {
        if let Some(ics) = data.text() {
            out.extend(ics_starts(ics));
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Startzeiten aller Termine eines iCalendar-Textes. Ganztägige und abgesagte
/// Termine zählen nicht; Zeiten ohne `Z` (Ortszeit) werden übersprungen, weil
/// der Server mit `expand` ohnehin UTC liefert.
pub fn ics_starts(ics: &str) -> Vec<u64> {
    // gefaltete Zeilen (Fortsetzung beginnt mit Leerzeichen oder Tab) zusammenfügen
    let unfolded = ics.replace("\r\n", "\n").replace("\n ", "").replace("\n\t", "");
    let mut out = vec![];
    let mut in_event = false;
    let mut start: Option<u64> = None;
    let mut cancelled = false;
    for line in unfolded.lines() {
        let (name, value) = match line.split_once(':') {
            Some(p) => p,
            None => continue,
        };
        let (key, params) = name.split_once(';').unwrap_or((name, ""));
        match (key.to_ascii_uppercase().as_str(), value.trim()) {
            ("BEGIN", "VEVENT") => {
                in_event = true;
                start = None;
                cancelled = false;
            }
            ("END", "VEVENT") => {
                if let (Some(s), false) = (start, cancelled) {
                    out.push(s);
                }
                in_event = false;
            }
            ("DTSTART", v) if in_event => {
                let all_day = params.to_ascii_uppercase().contains("VALUE=DATE") && !params.to_ascii_uppercase().contains("VALUE=DATE-TIME");
                start = if all_day { None } else { parse_utc(v) };
            }
            ("STATUS", v) if in_event => cancelled = v.eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    out
}

/// `20261105T140000Z` → Unix-Sekunden
pub fn parse_utc(v: &str) -> Option<u64> {
    let v = v.strip_suffix('Z')?;
    if v.len() != 15 || v.as_bytes()[8] != b'T' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| v.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(4..6)?, num(6..8)?);
    let (h, mi, s) = (num(9..11)?, num(11..13)?, num(13..15)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s;
    u64::try_from(secs).ok()
}

/// Unix-Sekunden → `20261105T140000Z`
pub fn format_utc(t: u64) -> String {
    let days = (t / 86400) as i64;
    let rem = t % 86400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

// Kalenderrechnung nach Howard Hinnant, „chrono-Compatible Low-Level Date Algorithms“
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

fn report_body(from: u64, to: u64) -> String {
    let (s, e) = (format_utc(from), format_utc(to));
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-data><c:expand start="{s}" end="{e}"/></c:calendar-data></d:prop>
  <c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT">
    <c:time-range start="{s}" end="{e}"/>
  </c:comp-filter></c:comp-filter></c:filter>
</c:calendar-query>"#
    )
}

const PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;

const HOME: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;

const CALENDARS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><d:resourcetype/><c:supported-calendar-component-set/></d:prop>
</d:propfind>"#;

struct Client {
    agent: ureq::Agent,
    auth: String,
}

impl Client {
    fn new(nc: &Nextcloud) -> Client {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .allow_non_standard_methods(true)
            .http_status_as_error(false)
            .build()
            .into();
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", nc.user.trim(), nc.password.trim()));
        Client { agent, auth: format!("Basic {token}") }
    }

    fn call(&self, method: &str, url: &str, depth: &str, body: &str) -> Result<String, String> {
        let req = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("Authorization", &self.auth)
            .header("Depth", depth)
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body.to_string())
            .map_err(|e| e.to_string())?;
        let mut resp = self.agent.run(req).map_err(|e| tr!("Nextcloud nicht erreichbar: {e}", "Nextcloud unreachable: {e}"))?;
        match resp.status().as_u16() {
            200..=299 => resp.body_mut().read_to_string().map_err(|e| format!("Nextcloud: {e}")),
            401 => Err(tr!("Nextcloud: Benutzer oder App-Passwort falsch", "Nextcloud: wrong user or app password")),
            404 => Err(tr!("Nextcloud: Kalender nicht gefunden (Serveradresse prüfen)", "Nextcloud: calendars not found (check the server address)")),
            s => Err(tr!("Nextcloud antwortet mit Status {s}", "Nextcloud replies with status {s}")),
        }
    }

    /// Adressen aller Termin-Kalender des Benutzers
    fn calendars(&self, server: &str) -> Result<Vec<String>, String> {
        check_secure(server)?;
        let root = format!("{server}/remote.php/dav/");
        let principal = href_of(&self.call("PROPFIND", &root, "0", PRINCIPAL)?, "current-user-principal")?
            .ok_or_else(|| tr!("Nextcloud: kein Benutzerkonto gefunden", "Nextcloud: no user account found"))?;
        let principal = resolve(server, &principal)?;
        let home = href_of(&self.call("PROPFIND", &principal, "0", HOME)?, "calendar-home-set")?
            .ok_or_else(|| tr!("Nextcloud: kein Kalenderordner gefunden", "Nextcloud: no calendar folder found"))?;
        let home = resolve(server, &home)?;
        calendars(&self.call("PROPFIND", &home, "1", CALENDARS)?)?.into_iter().map(|h| resolve(server, &h)).collect()
    }
}

/// Zahl der Termin-Kalender, für „Verbindung prüfen“ in den Einstellungen
pub fn check(nc: &Nextcloud) -> Result<usize, String> {
    Client::new(nc).calendars(&normalize(&nc.server)).map(|c| c.len())
}

/// Startzeiten der Termine zwischen `now` und `now + HORIZON`
pub fn fetch(nc: &Nextcloud, now: u64) -> Result<Vec<u64>, String> {
    let client = Client::new(nc);
    let body = report_body(now, now + HORIZON);
    let mut out = vec![];
    for cal in client.calendars(&normalize(&nc.server))? {
        out.extend(starts(&client.call("REPORT", &cal, "1", &body)?)?);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME_LISTING: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:nc="http://nextcloud.com/ns">
 <d:response>
  <d:href>/remote.php/dav/calendars/frank/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/calendars/frank/personal/</d:href>
  <d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <cal:supported-calendar-component-set><cal:comp name="VEVENT"/><cal:comp name="VTODO"/></cal:supported-calendar-component-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/calendars/frank/aufgaben/</d:href>
  <d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <cal:supported-calendar-component-set><cal:comp name="VTODO"/></cal:supported-calendar-component-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/calendars/frank/feiertage/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cs:subscribed/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/calendars/frank/inbox/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cal:schedule-inbox/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/calendars/frank/trashbin/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><nc:trash-bin/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
 </d:response>
</d:multistatus>"#;

    #[test]
    fn finds_only_event_calendars() {
        assert_eq!(calendars(HOME_LISTING).unwrap(), vec!["/remote.php/dav/calendars/frank/personal/"]);
    }

    #[test]
    fn reads_principal_and_home() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/remote.php/dav/principals/users/frank/</d:href>
<d:propstat><d:prop><cal:calendar-home-set><d:href>/remote.php/dav/calendars/frank/</d:href></cal:calendar-home-set></d:prop>
<d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
        assert_eq!(href_of(xml, "calendar-home-set").unwrap().as_deref(), Some("/remote.php/dav/calendars/frank/"));
        assert_eq!(href_of(xml, "current-user-principal").unwrap(), None);
    }

    #[test]
    fn reads_start_times_from_report() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
 <d:response><d:href>/remote.php/dav/calendars/frank/personal/a.ics</d:href>
  <d:propstat><d:prop><cal:calendar-data>BEGIN:VCALENDAR&#13;
VERSION:2.0&#13;
BEGIN:VEVENT&#13;
UID:a&#13;
DTSTART:20261105T140000Z&#13;
DTEND:20261105T150000Z&#13;
SUMMARY:Call&#13;
END:VEVENT&#13;
BEGIN:VEVENT&#13;
UID:a&#13;
RECURRENCE-ID:20261105T143000Z&#13;
DTSTART:20261105T143000Z&#13;
END:VEVENT&#13;
END:VCALENDAR&#13;
</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/frank/personal/b.ics</d:href>
  <d:propstat><d:prop><cal:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
DTSTART;VALUE=DATE:20261105
END:VEVENT
BEGIN:VEVENT
DTSTART:20261105T141500Z
STATUS:CANCELLED
END:VEVENT
END:VCALENDAR
</cal:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
        let t = parse_utc("20261105T140000Z").unwrap();
        assert_eq!(starts(xml).unwrap(), vec![t, t + 1800]);
    }

    #[test]
    fn folded_lines_and_local_times() {
        let ics = "BEGIN:VEVENT\nDTSTART;TZID=Europe/Berlin:20261105T150000\nEND:VEVENT\nBEGIN:VEVENT\nDTST\n ART:20261105T140000Z\nEND:VEVENT\n";
        assert_eq!(ics_starts(ics), vec![parse_utc("20261105T140000Z").unwrap()]);
    }

    #[test]
    fn utc_round_trip() {
        assert_eq!(parse_utc("19700101T000000Z"), Some(0));
        assert_eq!(parse_utc("20000301T000000Z"), Some(951868800));
        assert_eq!(format_utc(1793887200), "20261105T140000Z");
        assert_eq!(parse_utc("20261105T140000Z"), Some(1793887200));
        assert_eq!(parse_utc("20261105T140000"), None);
        assert_eq!(parse_utc("20261305T140000Z"), None);
    }

    #[test]
    fn server_address() {
        assert_eq!(normalize(" staude.cloud/ "), "https://staude.cloud");
        assert_eq!(normalize("https://example.org/nextcloud/"), "https://example.org/nextcloud");
        assert_eq!(
            resolve("https://example.org/nextcloud", "/nextcloud/remote.php/dav/calendars/f/").unwrap(),
            "https://example.org/nextcloud/remote.php/dav/calendars/f/"
        );
    }

    #[test]
    fn credentials_only_over_https_and_same_server() {
        assert!(check_secure("https://cloud.example.org").is_ok());
        assert!(check_secure("http://localhost:8080").is_ok());
        assert!(check_secure("http://cloud.example.org").is_err());
        assert!(check_secure("http://localhost.evil.org").is_err());
        assert!(resolve("https://a.org", "https://a.org/x/").is_ok());
        assert!(resolve("https://a.org", "https://b.org/x/").is_err());
    }

    #[test]
    fn password_not_in_debug_output() {
        let nc = Nextcloud { server: "s".into(), user: "u".into(), password: "geheim".into() };
        assert!(!format!("{nc:?}").contains("geheim"));
    }
}
