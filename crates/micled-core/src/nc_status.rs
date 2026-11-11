//! Nextcloud-Status bei „Auf Sendung“: Während eines Calls steht im eigenen
//! Benutzerstatus z. B. 🔴 „Im Call“, danach ist die Nachricht wieder leer.
//! Auf Wunsch steht dazu „Nicht stören“, danach kommt der vorherige Status.
//!
//! Wie beim Slack-Status überschreibt MicLED keine vorhandene Nachricht,
//! weder eine eigene („Homeoffice“) noch eine von Talk („In einem Anruf“).
//! Die eigene Nachricht erkennt MicLED am eingestellten Text. Als
//! Sicherheitsnetz läuft sie nach vier Stunden von selbst ab.

use std::time::Duration;

use serde_json::{json, Value};

use crate::caldav::{basic_auth, check_secure, normalize, Nextcloud};
use crate::presence::{plan, Presence};

/// Ablauf der eigenen Nachricht, falls MicLED sie nicht mehr zurücksetzen kann
const EXPIRES_AFTER: u64 = 4 * 3600;
const API: &str = "/ocs/v2.php/apps/user_status/api/v1/user_status";

/// Einstellung für den Status: Zugang, Text, Emoji und ob „Nicht stören“ dazukommt
#[derive(Clone, PartialEq, Eq)]
pub struct NcStatus {
    pub nc: Nextcloud,
    pub text: String,
    pub emoji: String,
    pub dnd: bool,
}

impl std::fmt::Debug for NcStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Nextcloud` lässt das Passwort selbst weg
        f.debug_struct("NcStatus")
            .field("nc", &self.nc)
            .field("text", &self.text)
            .field("emoji", &self.emoji)
            .field("dnd", &self.dnd)
            .finish()
    }
}

/// Aktueller Benutzerstatus laut Nextcloud
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Current {
    /// `online`, `away`, `dnd`, `invisible`, `offline` …
    pub status: String,
    /// Status selbst gewählt (sonst automatisch nach Aktivität)
    pub user_defined: bool,
    pub message: String,
    pub icon: String,
    /// Vordefinierte Nachricht (z. B. `call` von Talk), `None` bei eigener
    pub message_id: Option<String>,
}

impl Default for Current {
    fn default() -> Self {
        Current { status: "online".into(), user_defined: false, message: String::new(), icon: String::new(), message_id: None }
    }
}

/// Status von vorher, für „Nicht stören“
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub status: String,
    pub user_defined: bool,
}

/// Was beim Wechsel von „Auf Sendung“ zu tun ist
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Eigene Nachricht setzen und den bisherigen Status merken
    Set { save: Saved },
    /// Eigene Nachricht löschen; gemerkter Status, falls bekannt
    Restore(Option<Saved>),
    Nothing,
}

/// Entscheidung aus aktuellem Status, gewünschtem Zustand und gemerktem Status
pub fn decide(live: bool, current: &Current, ours: &str, saved: Option<&Saved>) -> Action {
    let is_ours = !ours.is_empty() && current.message_id.is_none() && current.message == ours;
    let empty = current.message.is_empty() && current.icon.is_empty() && current.message_id.is_none();
    match (live, is_ours) {
        (true, true) => Action::Nothing,
        (true, false) if empty => Action::Set { save: Saved { status: current.status.clone(), user_defined: current.user_defined } },
        (true, false) => Action::Nothing,
        (false, true) => Action::Restore(saved.cloned()),
        (false, false) => Action::Nothing,
    }
}

/// Status, auf den nach dem Call zurückgestellt wird (`None` = nichts tun).
/// Nur wenn „Nicht stören“ eingestellt ist und noch gilt: Ein vorher selbst
/// gewählter Status „Abwesend“, „Unsichtbar“ oder „Nicht stören“ kommt
/// zurück, sonst „Online“.
pub fn restore_status(dnd: bool, current: &Current, saved: Option<&Saved>) -> Option<String> {
    if !dnd || current.status != "dnd" {
        return None;
    }
    let kept = saved.filter(|s| s.user_defined && matches!(s.status.as_str(), "away" | "invisible" | "dnd"));
    Some(kept.map(|s| s.status.clone()).unwrap_or_else(|| "online".into()))
}

/// `ocs.data` aus der Antwort auf `GET …/user_status`; `null` zählt als leer
pub fn parse_current(v: &Value) -> Current {
    let d = &v["ocs"]["data"];
    let text = |k: &str| d[k].as_str().unwrap_or_default().to_string();
    Current {
        status: d["status"].as_str().filter(|s| !s.is_empty()).unwrap_or("online").to_string(),
        user_defined: d["statusIsUserDefined"].as_bool().unwrap_or(false),
        message: text("message"),
        icon: text("icon"),
        message_id: d["messageId"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
    }
}

/// Fehlermeldung zu einem HTTP-Status
pub fn error_for(code: u16) -> String {
    match code {
        401 => "Nextcloud: Benutzer oder App-Passwort falsch".into(),
        400 => "Nextcloud: Statustext oder Emoji ungültig".into(),
        404 => "Nextcloud: Benutzerstatus nicht verfügbar".into(),
        c => format!("Nextcloud-Status: Antwort {c}"),
    }
}

pub fn message_body(cfg: &NcStatus, now: u64) -> Value {
    let icon = (!cfg.emoji.is_empty()).then(|| cfg.emoji.clone());
    json!({ "statusIcon": icon, "message": cfg.text, "clearAt": now + EXPIRES_AFTER })
}

/// Zugriff auf die Status-API, in den Tests ersetzt
trait Api {
    fn current(&mut self, nc: &Nextcloud) -> Result<Current, String>;
    fn set_message(&mut self, cfg: &NcStatus, now: u64) -> Result<(), String>;
    fn clear_message(&mut self, nc: &Nextcloud) -> Result<(), String>;
    fn set_status(&mut self, nc: &Nextcloud, status: &str) -> Result<(), String>;
}

struct Http;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).http_status_as_error(false).build().into()
}

/// Anfrage senden; liefert Status-Code und Antwort als JSON (falls lesbar)
fn send(nc: &Nextcloud, method: &str, path: &str, body: Option<Value>) -> Result<(u16, Value), String> {
    let server = normalize(&nc.server);
    check_secure(&server)?;
    let url = format!("{server}{API}{path}");
    let auth = basic_auth(nc);
    let headers = [("Authorization", auth.as_str()), ("OCS-APIRequest", "true"), ("Accept", "application/json")];
    let agent = agent();
    let resp = match (method, body) {
        ("PUT", Some(b)) => headers.iter().fold(agent.put(&url), |r, (k, v)| r.header(*k, *v)).send_json(b),
        ("DELETE", _) => headers.iter().fold(agent.delete(&url), |r, (k, v)| r.header(*k, *v)).call(),
        _ => headers.iter().fold(agent.get(&url), |r, (k, v)| r.header(*k, *v)).call(),
    };
    let mut resp = resp.map_err(|e| format!("Nextcloud nicht erreichbar: {e}"))?;
    let code = resp.status().as_u16();
    let v = resp.body_mut().read_json::<Value>().unwrap_or(Value::Null);
    Ok((code, v))
}

fn expect_ok(r: (u16, Value)) -> Result<(), String> {
    match r.0 {
        200..=299 => Ok(()),
        c => Err(error_for(c)),
    }
}

impl Api for Http {
    fn current(&mut self, nc: &Nextcloud) -> Result<Current, String> {
        match send(nc, "GET", "", None)? {
            (200..=299, v) => Ok(parse_current(&v)),
            // noch nie ein Status gesetzt: Nextcloud antwortet mit 404 im OCS-Format
            (404, v) if v.get("ocs").is_some() => Ok(Current::default()),
            (c, _) => Err(error_for(c)),
        }
    }

    fn set_message(&mut self, cfg: &NcStatus, now: u64) -> Result<(), String> {
        expect_ok(send(&cfg.nc, "PUT", "/message/custom", Some(message_body(cfg, now)))?)
    }

    fn clear_message(&mut self, nc: &Nextcloud) -> Result<(), String> {
        expect_ok(send(nc, "DELETE", "/message", None)?)
    }

    fn set_status(&mut self, nc: &Nextcloud, status: &str) -> Result<(), String> {
        expect_ok(send(nc, "PUT", "/status", Some(json!({ "statusType": status })))?)
    }
}

/// Status bei Nextcloud, für [`crate::presence::Updater`]
#[derive(Default)]
pub struct State {
    /// Einstellung, mit der die eigene Nachricht gesetzt wurde
    ours: Option<NcStatus>,
    /// Status von vorher (`None` nach einem Absturz: unbekannt)
    saved: Option<Saved>,
    /// Eigene Nachricht steht noch und wird beim Zurückstellen gelöscht
    message: bool,
    /// MicLED hat „Nicht stören“ gesetzt und stellt es danach zurück
    dnd: bool,
    /// „Nicht stören“ steht noch aus (z. B. nach einem Fehler)
    dnd_todo: bool,
}

/// Steht die eigene Nachricht von `cfg`?
fn shows_ours(current: &Current, cfg: &NcStatus) -> bool {
    matches!(decide(false, current, &cfg.text, None), Action::Restore(_))
}

impl State {
    fn apply_with(&mut self, api: &mut impl Api, wanted: Option<&NcStatus>, live: bool, now: u64, startup: bool) -> Result<(), String> {
        let (restore, set) = plan(self.ours.as_ref(), wanted, live);
        let leftover = restore.is_none() && startup && !live;
        if let Some(mut cfg) = restore.or_else(|| wanted.filter(|_| leftover).cloned()) {
            // ein im Call neu eingetragenes App-Passwort desselben Kontos gilt auch fürs Zurückstellen
            if let Some(w) = wanted.filter(|w| normalize(&w.nc.server) == normalize(&cfg.nc.server) && w.nc.user.trim() == cfg.nc.user.trim()) {
                cfg.nc = w.nc.clone();
            }
            let current = api.current(&cfg.nc)?;
            if leftover {
                // nach einem Absturz: nur aufräumen, was sicher von MicLED stammt
                if !shows_ours(&current, &cfg) {
                    return Ok(());
                }
                self.ours = Some(cfg.clone());
                self.message = true;
                self.dnd = cfg.dnd;
            }
            if self.message && shows_ours(&current, &cfg) {
                api.clear_message(&cfg.nc)?;
            }
            self.message = false;
            self.dnd_todo = false;
            // auch wenn die Nachricht inzwischen abgelaufen oder ersetzt ist
            if self.dnd {
                if let Some(s) = restore_status(true, &current, self.saved.as_ref()) {
                    api.set_status(&cfg.nc, &s)?;
                }
            }
            self.dnd = false;
            self.ours = None;
            self.saved = None;
        }
        let mut current = None;
        if let Some(cfg) = set {
            let now_status = api.current(&cfg.nc)?;
            match decide(true, &now_status, &cfg.text, None) {
                Action::Set { save } => {
                    api.set_message(&cfg, now)?;
                    // ab hier ist die Nachricht gesetzt und wird beim Zurückstellen gelöscht
                    self.saved = Some(save);
                    self.message = true;
                    self.dnd_todo = cfg.dnd;
                    self.ours = Some(cfg);
                }
                // Neustart nach einem Absturz mitten im Call: eigene Nachricht übernehmen
                Action::Nothing if startup && shows_ours(&now_status, &cfg) => {
                    self.saved = None;
                    self.message = true;
                    self.dnd = cfg.dnd && now_status.status == "dnd";
                    self.dnd_todo = cfg.dnd && !self.dnd;
                    self.ours = Some(cfg);
                }
                _ => {}
            }
            current = Some(now_status);
        }
        if let Some(cfg) = self.ours.clone().filter(|_| self.dnd_todo) {
            let status = match current {
                Some(c) => c.status,
                None => api.current(&cfg.nc)?.status,
            };
            if status != "dnd" {
                api.set_status(&cfg.nc, "dnd")?;
                self.dnd = true;
            }
            self.dnd_todo = false;
        }
        Ok(())
    }
}

impl Presence for State {
    type Cfg = NcStatus;

    fn apply(&mut self, wanted: Option<&NcStatus>, live: bool, now: u64, startup: bool) -> Result<(), String> {
        self.apply_with(&mut Http, wanted, live, now, startup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cur(status: &str, message: &str) -> Current {
        Current { status: status.into(), message: message.into(), ..Current::default() }
    }

    fn saved(status: &str, user_defined: bool) -> Saved {
        Saved { status: status.into(), user_defined }
    }

    #[test]
    fn sets_only_over_an_empty_message() {
        assert_eq!(decide(true, &cur("away", ""), "Im Call", None), Action::Set { save: saved("away", false) });
        assert_eq!(decide(true, &cur("online", "Homeoffice"), "Im Call", None), Action::Nothing);
        assert_eq!(decide(true, &cur("online", "Im Call"), "Im Call", None), Action::Nothing, "schon gesetzt");
        let icon_only = Current { icon: "🏠".into(), ..Current::default() };
        assert_eq!(decide(true, &icon_only, "Im Call", None), Action::Nothing);
    }

    #[test]
    fn talk_call_message_stays() {
        let talk = Current { message: "Im Call".into(), message_id: Some("call".into()), ..Current::default() };
        assert_eq!(decide(true, &talk, "Im Call", None), Action::Nothing);
        assert_eq!(decide(false, &talk, "Im Call", Some(&saved("online", false))), Action::Nothing, "nicht unsere");
    }

    #[test]
    fn restores_only_its_own_message() {
        let s = saved("away", true);
        assert_eq!(decide(false, &cur("dnd", "Im Call"), "Im Call", Some(&s)), Action::Restore(Some(s.clone())));
        assert_eq!(decide(false, &cur("dnd", "Mittag"), "Im Call", Some(&s)), Action::Nothing, "inzwischen selbst geändert");
        assert_eq!(decide(false, &cur("online", ""), "Im Call", None), Action::Nothing);
        assert_eq!(decide(false, &cur("online", ""), "", None), Action::Nothing, "leerer Text passt nie");
    }

    #[test]
    fn leftover_message_after_crash_is_cleared() {
        assert_eq!(decide(false, &cur("dnd", "Im Call"), "Im Call", None), Action::Restore(None));
    }

    #[test]
    fn status_after_the_call() {
        let dnd = cur("dnd", "");
        assert_eq!(restore_status(false, &dnd, Some(&saved("online", false))), None, "ohne Schalter nichts");
        assert_eq!(restore_status(true, &cur("online", ""), Some(&saved("away", true))), None, "inzwischen geändert");
        assert_eq!(restore_status(true, &dnd, Some(&saved("online", false))).as_deref(), Some("online"));
        assert_eq!(restore_status(true, &dnd, Some(&saved("away", false))).as_deref(), Some("online"), "automatisch abwesend");
        assert_eq!(restore_status(true, &dnd, Some(&saved("away", true))).as_deref(), Some("away"));
        assert_eq!(restore_status(true, &dnd, Some(&saved("invisible", true))).as_deref(), Some("invisible"));
        assert_eq!(restore_status(true, &dnd, Some(&saved("dnd", true))).as_deref(), Some("dnd"));
        assert_eq!(restore_status(true, &dnd, Some(&saved("online", true))).as_deref(), Some("online"));
        assert_eq!(restore_status(true, &dnd, None).as_deref(), Some("online"), "nach einem Absturz");
    }

    #[test]
    fn parses_status_with_null_fields() {
        let v = json!({"ocs": {"meta": {"status": "ok"}, "data": {
            "userId": "frank", "message": null, "messageId": null, "messageIsPredefined": false,
            "icon": null, "clearAt": null, "status": "away", "statusIsUserDefined": true
        }}});
        assert_eq!(parse_current(&v), Current { status: "away".into(), user_defined: true, ..Current::default() });
        let talk = json!({"ocs": {"data": {"message": "In einem Anruf", "messageId": "call", "icon": "📞", "status": "online"}}});
        let c = parse_current(&talk);
        assert_eq!((c.message.as_str(), c.message_id.as_deref(), c.icon.as_str()), ("In einem Anruf", Some("call"), "📞"));
    }

    #[test]
    fn message_body_and_errors() {
        let mut cfg = config(false);
        let v = message_body(&cfg, 100);
        assert_eq!(v, json!({"statusIcon": "🔴", "message": "Im Call", "clearAt": 100 + 4 * 3600}));
        cfg.emoji.clear();
        assert_eq!(message_body(&cfg, 0)["statusIcon"], Value::Null);
        assert_eq!(error_for(401), "Nextcloud: Benutzer oder App-Passwort falsch");
        assert_eq!(error_for(500), "Nextcloud-Status: Antwort 500");
    }

    #[test]
    fn password_not_in_debug_output() {
        assert!(!format!("{:?}", config(true)).contains("geheim"));
    }

    fn config(dnd: bool) -> NcStatus {
        NcStatus {
            nc: Nextcloud { server: "https://cloud.example.org".into(), user: "frank".into(), password: "geheim".into() },
            text: "Im Call".into(),
            emoji: "🔴".into(),
            dnd,
        }
    }

    /// Nextcloud zum Mitschreiben: hält den Status und protokolliert Änderungen
    struct Fake {
        now: Current,
        log: Vec<String>,
        /// nächstes `set_status` scheitert
        fail_status: bool,
    }

    impl Fake {
        fn new(now: Current) -> Fake {
            Fake { now, log: vec![], fail_status: false }
        }
    }

    impl Api for Fake {
        fn current(&mut self, _: &Nextcloud) -> Result<Current, String> {
            Ok(self.now.clone())
        }
        fn set_message(&mut self, cfg: &NcStatus, _: u64) -> Result<(), String> {
            self.now.message = cfg.text.clone();
            self.now.icon = cfg.emoji.clone();
            self.log.push(format!("message {}", cfg.text));
            Ok(())
        }
        fn clear_message(&mut self, _: &Nextcloud) -> Result<(), String> {
            self.now.message.clear();
            self.now.icon.clear();
            self.log.push("clear".into());
            Ok(())
        }
        fn set_status(&mut self, _: &Nextcloud, status: &str) -> Result<(), String> {
            if std::mem::take(&mut self.fail_status) {
                return Err("Nextcloud nicht erreichbar".into());
            }
            self.now.status = status.into();
            self.now.user_defined = true;
            self.log.push(format!("status {status}"));
            Ok(())
        }
    }

    #[test]
    fn call_sets_and_restores_message_and_dnd() {
        let cfg = config(true);
        let mut api = Fake::new(Current { status: "away".into(), user_defined: true, ..Current::default() });
        let mut st = State::default();
        st.apply_with(&mut api, Some(&cfg), true, 0, true).unwrap();
        assert_eq!(api.log, ["message Im Call", "status dnd"]);
        st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
        assert_eq!(api.log.len(), 2, "schon gesetzt");
        st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
        assert_eq!(api.log, ["message Im Call", "status dnd", "clear", "status away"]);
        assert_eq!((api.now.message.as_str(), api.now.status.as_str()), ("", "away"));
    }

    #[test]
    fn without_dnd_only_the_message_changes() {
        let cfg = config(false);
        let mut api = Fake::new(Current::default());
        let mut st = State::default();
        st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
        st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
        assert_eq!(api.log, ["message Im Call", "clear"]);
    }

    #[test]
    fn own_message_is_never_touched() {
        let cfg = config(true);
        let mut api = Fake::new(cur("online", "Homeoffice"));
        let mut st = State::default();
        st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
        st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
        assert!(api.log.is_empty(), "{:?}", api.log);
    }

    #[test]
    fn startup_clears_leftover_from_a_crash() {
        let cfg = config(true);
        let mut api = Fake::new(cur("dnd", "Im Call"));
        State::default().apply_with(&mut api, Some(&cfg), false, 0, true).unwrap();
        assert_eq!(api.log, ["clear", "status online"]);
    }

    #[test]
    fn switching_off_during_call_restores() {
        let cfg = config(false);
        let mut api = Fake::new(Current::default());
        let mut st = State::default();
        st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
        st.apply_with(&mut api, None, true, 0, false).unwrap();
        assert_eq!(api.log, ["message Im Call", "clear"]);
    }

    #[test]
    fn dnd_is_restored_even_if_the_message_is_gone() {
        let cfg = config(true);
        for after in ["", "Mittag"] {
            let mut api = Fake::new(Current::default());
            let mut st = State::default();
            st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
            api.now.message = after.into();
            st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
            assert_eq!(api.log, ["message Im Call", "status dnd", "status online"], "Nachricht „{after}“");
            assert_eq!(api.now.message, after, "fremde Nachricht bleibt");
        }
    }

    #[test]
    fn failed_status_reset_is_retried() {
        let cfg = config(true);
        let mut api = Fake::new(Current::default());
        let mut st = State::default();
        st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
        api.fail_status = true;
        assert!(st.apply_with(&mut api, Some(&cfg), false, 0, false).is_err());
        st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
        assert_eq!(api.log, ["message Im Call", "status dnd", "clear", "status online"]);
    }

    #[test]
    fn failed_dnd_is_retried() {
        let cfg = config(true);
        let mut api = Fake::new(Current::default());
        api.fail_status = true;
        let mut st = State::default();
        assert!(st.apply_with(&mut api, Some(&cfg), true, 0, false).is_err());
        st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
        assert_eq!(api.log, ["message Im Call", "status dnd"]);
        st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
        assert_eq!(api.now.status, "online");
    }

    #[test]
    fn own_dnd_before_the_call_stays() {
        let cfg = config(true);
        let mut api = Fake::new(Current { status: "dnd".into(), user_defined: true, ..Current::default() });
        let mut st = State::default();
        st.apply_with(&mut api, Some(&cfg), true, 0, false).unwrap();
        st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
        assert_eq!(api.log, ["message Im Call", "clear"]);
        assert_eq!(api.now.status, "dnd");
    }

    #[test]
    fn restart_during_a_call_takes_over_the_leftover() {
        let cfg = config(true);
        let mut api = Fake::new(cur("dnd", "Im Call"));
        let mut st = State::default();
        st.apply_with(&mut api, Some(&cfg), true, 0, true).unwrap();
        assert!(api.log.is_empty(), "{:?}", api.log);
        st.apply_with(&mut api, Some(&cfg), false, 0, false).unwrap();
        assert_eq!(api.log, ["clear", "status online"]);
    }

    #[test]
    fn startup_without_leftover_leaves_own_dnd() {
        let cfg = config(true);
        let mut api = Fake::new(Current { status: "dnd".into(), user_defined: true, ..Current::default() });
        State::default().apply_with(&mut api, Some(&cfg), false, 0, true).unwrap();
        assert!(api.log.is_empty(), "{:?}", api.log);
    }

    #[test]
    fn new_password_of_the_same_account_is_used_for_restore() {
        struct Pw(Vec<String>);
        impl Api for Pw {
            fn current(&mut self, nc: &Nextcloud) -> Result<Current, String> {
                self.0.push(nc.password.clone());
                Ok(cur("online", "Im Call"))
            }
            fn set_message(&mut self, _: &NcStatus, _: u64) -> Result<(), String> {
                Ok(())
            }
            fn clear_message(&mut self, nc: &Nextcloud) -> Result<(), String> {
                self.0.push(format!("clear {}", nc.password));
                Ok(())
            }
            fn set_status(&mut self, _: &Nextcloud, _: &str) -> Result<(), String> {
                Ok(())
            }
        }
        let old = config(false);
        let mut new = old.clone();
        new.nc.password = "neu".into();
        let mut st = State { ours: Some(old), message: true, ..State::default() };
        let mut api = Pw(vec![]);
        st.apply_with(&mut api, Some(&new), false, 0, false).unwrap();
        assert_eq!(api.0, ["neu", "clear neu"]);
    }
}
