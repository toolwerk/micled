//! Slack-Status bei „Auf Sendung“: Während eines Calls steht im eigenen Profil
//! z. B. 🔴 „Im Call“, danach kommt der vorherige Status zurück.
//!
//! Ein Status, den du selbst gesetzt hast („Urlaub“), wird nie überschrieben.
//! Den eigenen Status erkennt MicLED am eingestellten Text. Als Sicherheitsnetz
//! läuft er nach vier Stunden von selbst ab.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

const API: &str = "https://slack.com/api";
/// Ablauf des eigenen Status, falls MicLED ihn nicht mehr zurücksetzen kann
const EXPIRES_AFTER: u64 = 4 * 3600;

#[derive(Clone, PartialEq, Eq)]
pub struct Slack {
    /// User-Token (`xoxp-…`) mit `users.profile:read` und `users.profile:write`
    pub token: String,
    pub text: String,
    pub emoji: String,
}

impl std::fmt::Debug for Slack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slack").field("text", &self.text).field("emoji", &self.emoji).finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub text: String,
    pub emoji: String,
    pub expiration: u64,
}

/// Was beim Wechsel von „Auf Sendung“ zu tun ist
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Eigenen Status setzen und den bisherigen merken
    Set { save: Profile },
    /// Gemerkten (oder leeren) Status wiederherstellen
    Restore(Profile),
    Nothing,
}

/// Entscheidung aus aktuellem Status, gewünschtem Zustand und gemerktem Status
pub fn decide(live: bool, current: &Profile, ours: &str, saved: Option<&Profile>) -> Action {
    let is_ours = !ours.is_empty() && current.text == ours;
    match (live, is_ours) {
        (true, true) => Action::Nothing,
        (true, false) if current.text.is_empty() && current.emoji.is_empty() => {
            Action::Set { save: current.clone() }
        }
        (true, false) => Action::Nothing,
        (false, true) => Action::Restore(saved.cloned().unwrap_or_default()),
        (false, false) => Action::Nothing,
    }
}

pub fn payload(p: &Profile) -> Value {
    json!({ "profile": {
        "status_text": p.text,
        "status_emoji": p.emoji,
        "status_expiration": p.expiration,
    }})
}

/// Antwort von `users.profile.get`
pub fn parse_profile(v: &Value) -> Result<Profile, String> {
    check_ok(v)?;
    let p = &v["profile"];
    Ok(Profile {
        text: p["status_text"].as_str().unwrap_or_default().to_string(),
        emoji: p["status_emoji"].as_str().unwrap_or_default().to_string(),
        expiration: p["status_expiration"].as_u64().unwrap_or(0),
    })
}

fn check_ok(v: &Value) -> Result<(), String> {
    if v["ok"].as_bool() == Some(true) {
        return Ok(());
    }
    Err(match v["error"].as_str().unwrap_or("unbekannter Fehler") {
        "invalid_auth" | "not_authed" | "token_revoked" => "Slack: Token ungültig".to_string(),
        "missing_scope" => "Slack: Token fehlt users.profile:read oder users.profile:write".to_string(),
        e => format!("Slack: {e}"),
    })
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into()
}

fn call(token: &str, method: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{API}/{method}");
    let auth = format!("Bearer {}", token.trim());
    let resp = match body {
        Some(b) => agent().post(&url).header("Authorization", &auth).send_json(b),
        None => agent().get(&url).header("Authorization", &auth).call(),
    };
    let v: Value = resp
        .map_err(|e| format!("Slack nicht erreichbar: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("Slack: {e}"))?;
    check_ok(&v)?;
    Ok(v)
}

/// Token prüfen; liefert „Name im Workspace“ für die Einstellungen
pub fn check(token: &str) -> Result<String, String> {
    let v = call(token, "auth.test", Some(json!({})))?;
    Ok(format!("{} in {}", v["user"].as_str().unwrap_or("?"), v["team"].as_str().unwrap_or("?")))
}

/// Arbeitet Wechsel nacheinander in einem eigenen Thread ab, damit Slack die
/// Lampe nicht aufhält und sich Setzen und Zurücksetzen nicht überholen.
pub struct Updater {
    tx: Sender<(Slack, bool, u64)>,
}

impl Updater {
    pub fn start(error: Arc<Mutex<Option<String>>>) -> Updater {
        let (tx, rx) = channel::<(Slack, bool, u64)>();
        std::thread::Builder::new()
            .name("micled-slack".into())
            .spawn(move || {
                let mut saved: Option<Profile> = None;
                for (cfg, live, now) in rx {
                    let r = step(&cfg, live, now, &mut saved);
                    if let Err(e) = &r {
                        log::warn!("{e}");
                    }
                    *error.lock().unwrap() = r.err();
                }
            })
            .expect("Slack-Thread");
        Updater { tx }
    }

    /// Neuer Zustand „Auf Sendung“ (`now` in Unix-Sekunden)
    pub fn send(&self, cfg: Slack, live: bool, now: u64) {
        let _ = self.tx.send((cfg, live, now));
    }
}

fn step(cfg: &Slack, live: bool, now: u64, saved: &mut Option<Profile>) -> Result<(), String> {
    let current = parse_profile(&call(&cfg.token, "users.profile.get", None)?)?;
    match decide(live, &current, &cfg.text, saved.as_ref()) {
        Action::Set { save } => {
            let ours = Profile { text: cfg.text.clone(), emoji: cfg.emoji.clone(), expiration: now + EXPIRES_AFTER };
            call(&cfg.token, "users.profile.set", Some(payload(&ours)))?;
            *saved = Some(save);
        }
        Action::Restore(p) => {
            // ein inzwischen abgelaufener alter Status bleibt leer
            let p = if p.expiration != 0 && p.expiration <= now { Profile::default() } else { p };
            call(&cfg.token, "users.profile.set", Some(payload(&p)))?;
            *saved = None;
        }
        Action::Nothing => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Profile {
        Profile { text: text.into(), emoji: String::new(), expiration: 0 }
    }

    #[test]
    fn sets_only_over_an_empty_status() {
        assert_eq!(decide(true, &p(""), "Im Call", None), Action::Set { save: p("") });
        assert_eq!(decide(true, &p("Urlaub"), "Im Call", None), Action::Nothing);
        assert_eq!(decide(true, &p("Im Call"), "Im Call", None), Action::Nothing);
    }

    #[test]
    fn restores_only_its_own_status() {
        assert_eq!(decide(false, &p("Im Call"), "Im Call", Some(&p(""))), Action::Restore(p("")));
        assert_eq!(decide(false, &p("Mittag"), "Im Call", Some(&p(""))), Action::Nothing, "inzwischen selbst geändert");
        assert_eq!(decide(false, &p(""), "Im Call", None), Action::Nothing);
    }

    #[test]
    fn leftover_status_after_crash_is_cleared() {
        assert_eq!(decide(false, &p("Im Call"), "Im Call", None), Action::Restore(Profile::default()));
    }

    #[test]
    fn empty_own_text_never_matches() {
        assert_eq!(decide(false, &p(""), "", None), Action::Nothing);
    }

    #[test]
    fn parses_profile_and_errors() {
        let ok = json!({"ok": true, "profile": {"status_text": "Urlaub", "status_emoji": ":palm_tree:", "status_expiration": 5}});
        assert_eq!(
            parse_profile(&ok).unwrap(),
            Profile { text: "Urlaub".into(), emoji: ":palm_tree:".into(), expiration: 5 }
        );
        assert_eq!(parse_profile(&json!({"ok": false, "error": "invalid_auth"})).unwrap_err(), "Slack: Token ungültig");
    }

    #[test]
    fn payload_format() {
        let v = payload(&Profile { text: "Im Call".into(), emoji: ":red_circle:".into(), expiration: 9 });
        assert_eq!(v["profile"]["status_text"], "Im Call");
        assert_eq!(v["profile"]["status_expiration"], 9);
    }

    #[test]
    fn token_not_in_debug_output() {
        let s = Slack { token: "xoxp-geheim".into(), text: "t".into(), emoji: "e".into() };
        assert!(!format!("{s:?}").contains("geheim"));
    }
}
