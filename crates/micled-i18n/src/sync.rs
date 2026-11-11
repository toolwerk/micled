//! Vorlage und Sprachen nachführen (`export`) und eine ausgefüllte Datei
//! übernehmen (`import`). Beides rechnet nur und liefert die Dateien; geschrieben
//! wird in `main.rs`, damit die Tests den Stand im Repo vergleichen können.

use crate::json::J;
use crate::{app, core, po};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;

pub const TEMPLATE: &str = "translations/template.pot";
pub const LOCALES: &str = "crates/micled-core/src/locales.json";
const DIR: &str = "translations";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Core,
    App,
}

/// Ein Text der Vorlage
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: Kind,
    /// Kern: deutsches Original; App: `<Katalog>/<Schlüssel>`
    pub ctxt: String,
    /// Englisches Original
    pub id: String,
    pub hints: Vec<String>,
    pub refs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Translated,
    Missing,
    Fuzzy,
    Invalid(String),
}

/// Ergebnis: Dateien relativ zur Repo-Wurzel mit Inhalt, dazu eine Zeile je Sprache
#[derive(Debug, Default)]
pub struct Output {
    pub files: Vec<(String, String)>,
    pub summary: Vec<String>,
    /// je nicht übersetztem Text einer importierten Sprache: Kontext und Grund
    pub details: Vec<String>,
}

/// Platzhalter eines Textes für den Hinweis in der Vorlage, sortiert:
/// `{name}` (Kern), `%@`, `%lld` (App) und `${applicationName}` (Siri)
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out = names(s);
    out.extend(printf_specs(s).unwrap_or_default().into_iter().map(|(_, conv)| format!("%{conv}")));
    out.sort();
    out
}

/// Benannte Platzhalter `{name}` und `${name}`, sortiert
fn names(s: &str) -> Vec<String> {
    let re = Regex::new(r"\$\{[A-Za-z_][A-Za-z0-9_]*\}|\{\{|\}\}|\{[A-Za-z_][A-Za-z0-9_]*\}").expect("Regex");
    let mut out: Vec<String> = re.find_iter(s).map(|m| m.as_str()).filter(|m| *m != "{{" && *m != "}}").map(str::to_string).collect();
    out.sort();
    out
}

/// Formatangaben der App (`%@`, `%lld`, `%2$@`) als Position und Typ (Längen-
/// angabe und Umwandlung, ohne Breite). Jedes `%` muss eine bekannte Angabe
/// oder `%%` sein; `*`, `%n` und Unbekanntes sind ein Fehler, weil die App den
/// Text an `String(format:)` gibt.
fn printf_specs(s: &str) -> Result<Vec<(Option<usize>, String)>, String> {
    let re = Regex::new(r"^%(?:(\d+)\$)?[-+ #0']*\d*(?:\.\d+)?((?:hh|h|ll|l|q|z|t|j)?[@dDiuUxXoOfFeEgGcCsSpaA])").expect("Regex");
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = s[i..].find('%') {
        let at = i + off;
        let rest = &s[at..];
        if rest.starts_with("%%") {
            i = at + 2;
            continue;
        }
        let c = re.captures(rest).ok_or_else(|| format!("unknown format “{}”", rest.chars().take(6).collect::<String>()))?;
        let pos = match c.get(1) {
            Some(p) => Some(p.as_str().parse::<usize>().ok().filter(|p| *p > 0).ok_or("position 0")?),
            None => None,
        };
        out.push((pos, c[2].to_string()));
        i = at + c[0].len();
    }
    Ok(out)
}

/// Formatangaben nach Position: ohne `n$` in der Reihenfolge, gemischt ist
/// ein Fehler
fn by_position(specs: Vec<(Option<usize>, String)>) -> Result<BTreeMap<usize, String>, String> {
    let explicit = specs.iter().filter(|(p, _)| p.is_some()).count();
    if explicit != 0 && explicit != specs.len() {
        return Err("mixed numbered and plain formats".into());
    }
    let mut map = BTreeMap::new();
    for (n, (pos, conv)) in specs.into_iter().enumerate() {
        let pos = pos.unwrap_or(n + 1);
        if let Some(prev) = map.insert(pos, conv.clone()) {
            if prev != conv {
                return Err(format!("format {pos} used as %{prev} and %{conv}"));
            }
        }
    }
    Ok(map)
}

/// Prüft die Platzhalter einer Übersetzung gegen das Englische: dieselben
/// benannten; in der App dieselben Formatangaben mit Position und Typ
pub fn check_placeholders(kind: Kind, en: &str, text: &str) -> Result<(), String> {
    let (want, got) = (names(en), names(text));
    if want != got {
        return Err(format!("placeholders {} expected, found {}", show(&want), show(&got)));
    }
    if kind == Kind::Core {
        return Ok(()); // Kern-Texte gehen nicht durch printf
    }
    let want = by_position(printf_specs(en)?)?;
    let got = by_position(printf_specs(text)?)?;
    if want != got {
        let fmt = |m: &BTreeMap<usize, String>| m.iter().map(|(p, c)| format!("%{p}${c}")).collect::<Vec<_>>();
        return Err(format!("formats {} expected, found {}", show(&fmt(&want)), show(&fmt(&got))));
    }
    Ok(())
}

pub fn status(tpl: &Entry, e: Option<&po::Entry>) -> Status {
    let Some(e) = e.filter(|e| !e.text.trim().is_empty()) else {
        return Status::Missing;
    };
    if e.fuzzy() || e.id != tpl.id {
        return Status::Fuzzy;
    }
    match check_placeholders(tpl.kind, &tpl.id, &e.text) {
        Ok(()) => Status::Translated,
        Err(why) => Status::Invalid(why),
    }
}

fn show(p: &[String]) -> String {
    if p.is_empty() {
        "none".into()
    } else {
        p.join(" ")
    }
}

/// String Catalogs der App: Name und Inhalt
pub type Catalogs = Vec<(&'static str, J)>;

/// Alle Texte aus Kern und App, nach Kontext sortiert
pub fn template(root: &Path) -> Result<(Vec<Entry>, Catalogs), String> {
    // deutsches Original → (Eintrag, Bereiche, erste Fundstelle)
    let mut by_de: BTreeMap<String, (Entry, Vec<&str>, String)> = BTreeMap::new();
    for t in core::scan(root)? {
        let at = format!("{}:{}", t.file, t.line);
        let area = core::area(&t.file);
        if let Some((e, areas, first)) = by_de.get_mut(&t.de) {
            if e.id != t.en {
                return Err(format!("{at}: „{}“ hat dort ein anderes Englisch als in {first}", t.de));
            }
            if !areas.contains(&area) {
                areas.push(area);
            }
            if !e.refs.contains(&t.file) {
                e.refs.push(t.file.clone());
            }
            continue;
        }
        let e = Entry { kind: Kind::Core, ctxt: t.de.clone(), id: t.en, hints: Vec::new(), refs: vec![t.file] };
        by_de.insert(t.de, (e, vec![area], at));
    }
    let mut entries: Vec<Entry> = Vec::new();
    for (mut e, areas, _) in by_de.into_values() {
        let named: Vec<&str> = areas.into_iter().filter(|a| *a != "Error message").collect();
        let hint = if named.is_empty() { "Error message".to_string() } else { format!("Error message: {}", named.join(", ")) };
        e.hints = std::iter::once(hint).chain(app::keep_hint(&e.id)).collect();
        entries.push(e);
    }

    let mut catalogs = Vec::new();
    for (name, path) in app::CATALOGS {
        let text = read(root, path)?;
        catalogs.push((name, J::parse(&text).map_err(|e| format!("{path}: {e}"))?));
    }
    let swift = swift_sources(root)?;
    for t in app::texts(&catalogs, &swift)? {
        entries.push(Entry { kind: Kind::App, ctxt: t.ctxt, id: t.en, hints: t.hints, refs: t.refs });
    }
    entries.sort_by(|a, b| a.ctxt.cmp(&b.ctxt));
    if let Some(w) = entries.windows(2).find(|w| w[0].ctxt == w[1].ctxt) {
        return Err(format!("Text doppelt in Kern und App: {}", w[0].ctxt));
    }
    Ok((entries, catalogs))
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))
}

fn swift_sources(root: &Path) -> Result<Vec<(String, String)>, String> {
    let dir = root.join(app::SOURCES);
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", app::SOURCES))? {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|x| x == "swift") {
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            out.push((name, std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?));
        }
    }
    out.sort();
    Ok(out)
}

fn header(lang: &str) -> po::Entry {
    let text = format!(
        "Project-Id-Version: MicLED\nLanguage: {lang}\nMIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n"
    );
    po::Entry { text, ..Default::default() }
}

fn template_file(entries: &[Entry]) -> po::File {
    let mut h = header("");
    h.translator = vec![
        "MicLED translation template, generated by scripts/translations.sh.".into(),
        "Do not edit; copy it to <language code>.po and see TRANSLATING.md.".into(),
    ];
    po::File {
        header: Some(h),
        entries: entries
            .iter()
            .map(|e| po::Entry { extracted: e.hints.clone(), refs: e.refs.clone(), ctxt: Some(e.ctxt.clone()), id: e.id.clone(), ..Default::default() })
            .collect(),
    }
}

/// Wo ein Kontext hingehört: Katalog (oder Kern) und Pluralform
fn slot(ctxt: &str) -> (&str, &str) {
    let catalog = app::CATALOGS.iter().map(|(n, _)| *n).find(|n| ctxt.strip_prefix(n).is_some_and(|r| r.starts_with('/'))).unwrap_or("core");
    let plural = if catalog == "core" { "" } else { ctxt.rsplit_once('#').map_or("", |(_, p)| p) };
    (catalog, plural)
}

/// Führt eine Übersetzung auf den Stand der Vorlage: neue Texte leer,
/// geänderte als `fuzzy` mit dem alten Englisch in `#|`, entfallene raus.
/// Hat sich nur das Deutsche (der Kontext) geändert, wird die Übersetzung mit
/// gleichem Englisch übernommen und als `fuzzy` zur Prüfung markiert.
pub fn merge(entries: &[Entry], old: &po::File, lang: &str) -> po::File {
    let by_ctxt: BTreeMap<&str, &po::Entry> = old.entries.iter().filter_map(|e| e.ctxt.as_deref().map(|c| (c, e))).collect();
    let current: std::collections::BTreeSet<&str> = entries.iter().map(|e| e.ctxt.as_str()).collect();
    // entfallene Übersetzungen nach Ort und Englisch, je eine nur einmal vergeben
    let mut orphans: BTreeMap<((&str, &str), &str), &po::Entry> = BTreeMap::new();
    for o in &old.entries {
        let Some(c) = o.ctxt.as_deref() else { continue };
        if !current.contains(c) && !o.text.is_empty() {
            orphans.entry((slot(c), o.id.as_str())).or_insert(o);
        }
    }
    let mut header = old.header.clone().unwrap_or_else(|| header(lang));
    header.flags.retain(|f| f != "fuzzy");
    let mut file = po::File { header: Some(header), entries: Vec::new() };
    file.set_header_field("Language", lang);
    for t in entries {
        let mut e = po::Entry { extracted: t.hints.clone(), refs: t.refs.clone(), ctxt: Some(t.ctxt.clone()), id: t.id.clone(), ..Default::default() };
        if let Some(o) = by_ctxt.get(t.ctxt.as_str()) {
            e.translator = o.translator.clone();
            e.text = o.text.clone();
            e.flags = o.flags.iter().filter(|f| *f != "fuzzy").cloned().collect();
            if !o.text.is_empty() {
                if o.id != t.id {
                    e.set_fuzzy(true);
                    e.prev_id = Some(if o.fuzzy() { o.prev_id.clone().unwrap_or_else(|| o.id.clone()) } else { o.id.clone() });
                } else if o.fuzzy() {
                    e.set_fuzzy(true);
                    e.prev_id = o.prev_id.clone();
                }
            }
        } else if let Some(o) = orphans.remove(&(slot(&t.ctxt), t.id.as_str())) {
            e.translator = o.translator.clone();
            e.text = o.text.clone();
            e.flags = o.flags.iter().filter(|f| *f != "fuzzy").cloned().collect();
            e.set_fuzzy(true);
            e.prev_id = o.prev_id.clone();
        }
        file.entries.push(e);
    }
    file
}

/// Sprachcode prüfen und vereinheitlichen (`pt_BR` → `pt-BR`); Deutsch und
/// Englisch sind eingebaut
pub fn lang_code(raw: &str) -> Result<String, String> {
    let code = raw.trim().replace('_', "-");
    let ok = Regex::new(r"^[a-zA-Z]{2,3}(-[A-Za-z0-9]{2,8})*$").expect("Regex").is_match(&code);
    if !ok {
        return Err(format!("Sprachcode „{raw}“ ungültig, erwartet z. B. fr, es oder pt-BR"));
    }
    let mut parts = code.splitn(2, '-');
    let first = parts.next().unwrap_or_default().to_ascii_lowercase();
    if app::BUILT_IN.contains(&first.as_str()) {
        return Err(format!("{code}: Deutsch und Englisch sind eingebaut und kommen nicht aus einer Übersetzungsdatei"));
    }
    // wie Apple: Region groß (pt-BR), Schrift mit großem Anfang (zh-Hans)
    let mut code = first;
    for part in parts.next().unwrap_or_default().split('-').filter(|p| !p.is_empty()) {
        let part = match part.len() {
            4 => part[..1].to_ascii_uppercase() + &part[1..].to_ascii_lowercase(),
            2 => part.to_ascii_uppercase(),
            _ => part.to_ascii_lowercase(),
        };
        code.push('-');
        code.push_str(&part);
    }
    Ok(code)
}

/// Vorhandene Sprachen `translations/<code>.po`, nach Code sortiert; `skip`
/// (der gerade importierte Code) wird in jeder Schreibweise übergangen
fn languages(root: &Path, skip: Option<&str>) -> Result<Vec<(String, po::File)>, String> {
    let dir = root.join(DIR);
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(out) };
    for e in rd {
        let p = e.map_err(|e| e.to_string())?.path();
        if p.extension().is_none_or(|x| x != "po") {
            continue;
        }
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        if skip.is_some_and(|s| s.eq_ignore_ascii_case(&stem.replace('_', "-"))) {
            continue;
        }
        let rel = format!("{DIR}/{stem}.po");
        let code = lang_code(&stem).map_err(|e| format!("{rel}: {e}"))?;
        if code != stem {
            return Err(format!("{rel}: Datei muss {code}.po heißen (git mv {rel} {DIR}/{code}.po)"));
        }
        let file = po::parse(&read(root, &rel)?).map_err(|e| format!("{rel}: {e}"))?;
        out.push((code, file));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Rechnet alle Dateien aus dem Stand im Repo; `extra` ersetzt oder ergänzt
/// eine Sprache (Import)
pub fn export(root: &Path, extra: Option<(String, po::File)>) -> Result<Output, String> {
    let (entries, mut catalogs) = template(root)?;
    let mut langs = languages(root, extra.as_ref().map(|(c, _)| c.as_str()))?;
    if let Some((code, file)) = extra {
        langs.retain(|(c, _)| *c != code);
        langs.push((code, file));
        langs.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let mut out = Output::default();
    out.files.push((TEMPLATE.into(), po::write(&template_file(&entries))));

    let mut merged = Vec::new();
    for (code, old) in &langs {
        let file = merge(&entries, old, code);
        let by_ctxt: BTreeMap<&str, &po::Entry> = file.entries.iter().filter_map(|e| e.ctxt.as_deref().map(|c| (c, e))).collect();
        let mut table: BTreeMap<String, String> = BTreeMap::new();
        let mut app_texts: BTreeMap<String, String> = BTreeMap::new();
        let (mut done, mut missing, mut fuzzy, mut invalid) = (0, 0, 0, 0);
        for t in &entries {
            let e = by_ctxt.get(t.ctxt.as_str()).copied();
            match status(t, e) {
                Status::Translated => {
                    done += 1;
                    let text = e.map(|e| e.text.clone()).unwrap_or_default();
                    match t.kind {
                        Kind::Core => table.insert(t.ctxt.clone(), text),
                        Kind::App => app_texts.insert(t.ctxt.clone(), text),
                    };
                }
                s => {
                    let reason = match s {
                        Status::Missing => {
                            missing += 1;
                            "missing".to_string()
                        }
                        Status::Fuzzy => {
                            fuzzy += 1;
                            "fuzzy (English text changed, please check)".to_string()
                        }
                        Status::Invalid(why) => {
                            invalid += 1;
                            format!("invalid: {why}")
                        }
                        Status::Translated => unreachable!(),
                    };
                    out.details.push(format!("{code}: {}: {reason}", t.ctxt.replace('\n', "\\n")));
                }
            }
        }
        let mut line = format!("{code}: {done} translated, {missing} missing, {fuzzy} fuzzy");
        if invalid > 0 {
            line.push_str(&format!(", {invalid} invalid"));
        }
        out.summary.push(line);
        out.files.push((format!("{DIR}/{code}.po"), po::write(&file)));
        merged.push((code.clone(), table, app_texts));
    }

    let locales: BTreeMap<&str, &BTreeMap<String, String>> = merged.iter().map(|(c, t, _)| (c.as_str(), t)).collect();
    out.files.push((LOCALES.into(), serde_json::to_string_pretty(&locales).map_err(|e| e.to_string())? + "\n"));

    let lookups: Vec<(String, Box<app::Lookup>)> = merged
        .iter()
        .map(|(code, _, texts)| {
            let f: Box<app::Lookup> = Box::new(move |ctxt: &str| texts.get(ctxt).cloned());
            (code.clone(), f)
        })
        .collect();
    let langs: Vec<(String, &app::Lookup)> = lookups.iter().map(|(c, f)| (c.clone(), f.as_ref())).collect();
    for ((name, path), (_, cat)) in app::CATALOGS.iter().zip(catalogs.iter_mut()) {
        app::apply(name, cat, &langs)?;
        out.files.push((path.to_string(), cat.pretty()));
    }
    Ok(out)
}

/// Übernimmt eine ausgefüllte Datei als `translations/<code>.po`
pub fn import(root: &Path, text: &str, lang: Option<&str>) -> Result<(String, Output), String> {
    let mut file = po::parse(text)?;
    let raw = match lang {
        Some(l) => l.to_string(),
        None => file.header_field("Language").filter(|l| !l.is_empty()).ok_or("Sprache fehlt: Kopfzeile Language leer, bitte --lang <code> angeben")?,
    };
    let code = lang_code(&raw)?;
    file.set_header_field("Language", &code);
    let mut out = export(root, Some((code.clone(), file)))?;
    let prefix = format!("{code}: ");
    out.details.retain(|d| d.starts_with(&prefix));
    out.summary.retain(|s| s.starts_with(&prefix));
    Ok((code, out))
}

/// Liegt die Datei in anderer Schreibweise vor (`pt-br.po` statt `pt-BR.po`),
/// bekommt sie den richtigen Namen; auf macOS behielte sie ihn sonst beim Schreiben
fn rename_other_spelling(path: &Path) -> std::io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Ok(()) };
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(()) };
    for e in rd.flatten() {
        let other = e.file_name();
        if other != name && other.to_string_lossy().eq_ignore_ascii_case(&name.to_string_lossy()) {
            std::fs::rename(e.path(), path)?;
        }
    }
    Ok(())
}

/// Schreibt nur geänderte Dateien, damit Xcode nicht unnötig neu baut;
/// liefert die geschriebenen
pub fn write(root: &Path, out: &Output) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    for (rel, content) in &out.files {
        let path = root.join(rel);
        rename_other_spelling(&path).map_err(|e| format!("{rel}: {e}"))?;
        if std::fs::read_to_string(&path).ok().as_deref() == Some(content.as_str()) {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{rel}: {e}"))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("{rel}: {e}"))?;
        written.push(rel.clone());
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
    }

    fn entry(ctxt: &str, id: &str) -> Entry {
        Entry { kind: Kind::Core, ctxt: ctxt.into(), id: id.into(), hints: vec!["Error message: Pixoo".into()], refs: vec!["crates/micled-core/src/pixoo.rs".into()] }
    }

    fn po_entry(ctxt: &str, id: &str, text: &str) -> po::Entry {
        po::Entry { ctxt: Some(ctxt.into()), id: id.into(), text: text.into(), ..Default::default() }
    }

    /// Entscheidung 2: Vorlage, Sprachen, `locales.json` und Kataloge im Repo
    /// sind auf dem Stand der Quellen
    #[test]
    fn repo_files_are_up_to_date() {
        let root = root();
        let out = export(&root, None).unwrap();
        for (rel, content) in &out.files {
            let disk = std::fs::read_to_string(root.join(rel)).unwrap_or_default();
            assert!(disk == *content, "{rel} ist veraltet: run scripts/translations.sh");
        }
    }

    #[test]
    fn template_has_core_and_app_texts() {
        let (entries, _) = template(&root()).unwrap();
        let pixoo = entries.iter().find(|e| e.ctxt == "Pixoo meldet Fehler {code}").unwrap();
        assert_eq!(pixoo.id, "Pixoo reports error {code}");
        assert_eq!(pixoo.hints, ["Error message: Pixoo", "Keep the placeholders: {code}"]);
        let shared = entries.iter().find(|e| e.ctxt == "Nextcloud nicht erreichbar: {e}").unwrap();
        assert_eq!(shared.refs, ["crates/micled-core/src/caldav.rs", "crates/micled-core/src/nc_status.rs"]);
        assert!(entries.iter().any(|e| e.kind == Kind::App && e.ctxt.starts_with("InfoPlist/")));
        assert!(entries.iter().any(|e| e.ctxt.starts_with("AppShortcuts/") && e.id.contains("${applicationName}")));
        assert!(entries.iter().any(|e| e.ctxt.ends_with("#one")));
        assert!(entries.iter().all(|e| !e.id.is_empty() && !e.hints.is_empty()));
        for e in entries.iter().filter(|e| e.kind == Kind::App) {
            assert!(check_placeholders(Kind::App, &e.id, &e.id).is_ok(), "{}", e.ctxt);
        }
    }

    /// Entscheidung 3 und Erledigt 5: geändertes Englisch wird `fuzzy`, neue
    /// Texte leer, entfallene verschwinden
    #[test]
    fn changed_english_becomes_fuzzy() {
        let tpl = vec![entry("A", "Pixoo reports error {code}"), entry("B", "Same"), entry("N", "New")];
        let mut kept = po_entry("B", "Same", "Pareil");
        kept.translator = vec!["checked".into()];
        let old = po::File { header: None, entries: vec![po_entry("A", "Pixoo error {code}", "Erreur {code}"), kept, po_entry("Gone", "Old", "Vieux")] };
        let f = merge(&tpl, &old, "fr");
        assert_eq!(f.header_field("Language").as_deref(), Some("fr"));
        assert_eq!(f.entries.len(), 3);
        let a = &f.entries[0];
        assert!(a.fuzzy());
        assert_eq!((a.id.as_str(), a.text.as_str(), a.prev_id.as_deref()), ("Pixoo reports error {code}", "Erreur {code}", Some("Pixoo error {code}")));
        assert_eq!(status(&tpl[0], Some(a)), Status::Fuzzy);
        assert_eq!(f.entries[1].translator, ["checked"]);
        assert_eq!(status(&tpl[1], Some(&f.entries[1])), Status::Translated);
        assert_eq!((f.entries[2].text.as_str(), status(&tpl[2], Some(&f.entries[2]))), ("", Status::Missing));

        // ein zweiter Durchlauf behält fuzzy und das erste alte Englisch
        let again = merge(&tpl, &f, "fr");
        assert_eq!(again, f);
    }

    /// Nur das Deutsche korrigiert: Übersetzung bleibt, aber zur Prüfung
    #[test]
    fn changed_german_keeps_translation_as_fuzzy() {
        let mut moved = entry("Pixoo meldet einen Fehler {code}", "Pixoo reports error {code}");
        let app = Entry { kind: Kind::App, ..entry("Localizable/Beenden!", "Quit") };
        let other_catalog = Entry { kind: Kind::App, ..entry("InfoPlist/Neu", "Quit") };
        moved.kind = Kind::Core;
        let tpl = vec![moved.clone(), app.clone(), other_catalog.clone()];
        let old = po::File {
            header: None,
            entries: vec![po_entry("Pixoo meldet Fehler {code}", "Pixoo reports error {code}", "Erreur {code}"), po_entry("Localizable/Beenden", "Quit", "Quitter")],
        };
        let f = merge(&tpl, &old, "fr");
        assert_eq!((f.entries[0].text.as_str(), f.entries[0].fuzzy()), ("Erreur {code}", true));
        assert_eq!(status(&moved, Some(&f.entries[0])), Status::Fuzzy);
        assert_eq!((f.entries[1].text.as_str(), f.entries[1].fuzzy()), ("Quitter", true));
        assert_eq!(f.entries[2].text, "", "nur im selben Katalog und nur einmal");
    }

    #[test]
    fn placeholders_must_match() {
        let t = entry("Pixoo {host} meldet {code}", "Pixoo {host} reports {code}");
        assert_eq!(status(&t, Some(&po_entry("x", &t.id, "Pixoo {host} : {code}"))), Status::Translated);
        assert_eq!(status(&t, Some(&po_entry("x", &t.id, "{code} sur Pixoo {host}"))), Status::Translated);
        assert!(matches!(status(&t, Some(&po_entry("x", &t.id, "Pixoo {hote} : {code}"))), Status::Invalid(_)));
        let siri = Entry { kind: Kind::App, ..entry("AppShortcuts/x", "Turn off ${applicationName}") };
        assert!(matches!(status(&siri, Some(&po_entry("x", &siri.id, "Éteindre MicLED"))), Status::Invalid(_)));
        assert_eq!(placeholders("50 %% {{x}} %lld"), ["%lld"]);
        assert_eq!(placeholders("{e} ${app} %2$@"), ["${app}", "%@", "{e}"]);
    }

    /// Die App gibt Übersetzungen an `String(format:)`: Position und Typ jeder
    /// Formatangabe müssen zum Englischen passen, Unbekanntes ist ungültig
    #[test]
    fn app_formats_must_match_position_and_type() {
        let ok = |en: &str, t: &str| check_placeholders(Kind::App, en, t);
        assert!(ok("%@ of %lld", "%@ de %lld").is_ok());
        assert!(ok("%@ of %lld", "%2$lld de %1$@").is_ok());
        assert!(ok("%lld %%", "%lld %%").is_ok());
        assert!(ok("%@ of %lld", "%lld de %@").is_err(), "Typen vertauscht ohne Position");
        assert!(ok("%@ of %lld", "%@ de").is_err());
        assert!(ok("%@ of %lld", "%1$@ de %3$lld").is_err(), "Position außerhalb");
        assert!(ok("%@ of %lld", "%1$@ de %lld").is_err(), "gemischt");
        assert!(ok("%@", "%@ %n").is_err());
        assert!(ok("%lld", "%*lld").is_err());
        assert!(ok("%@", "%@ 50 %").is_err(), "einzelnes %");
        assert!(ok("%@", "%s").is_err());
        assert!(ok("Hi", "Salut %@").is_err());
        // im Kern ist % nur Text
        assert!(check_placeholders(Kind::Core, "50 % {e}", "50 % {e}").is_ok());
        assert!(check_placeholders(Kind::Core, "{e}", "{e} %n").is_ok());
    }

    #[test]
    fn language_codes() {
        assert_eq!(lang_code("fr").unwrap(), "fr");
        assert_eq!(lang_code("pt_BR").unwrap(), "pt-BR");
        assert_eq!(lang_code("FR").unwrap(), "fr");
        assert_eq!(lang_code("pt-br").unwrap(), "pt-BR");
        assert_eq!(lang_code("zh_hans").unwrap(), "zh-Hans");
        for bad in ["de", "de-AT", "en", "en_GB", "", "français", "../x"] {
            assert!(lang_code(bad).is_err(), "{bad}");
        }
    }

    fn copy_dir(from: &Path, to: &Path, keep: &dyn Fn(&Path) -> bool) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let p = e.unwrap().path();
            let target = to.join(p.file_name().unwrap());
            if p.is_dir() {
                copy_dir(&p, &target, keep);
            } else if keep(&p) {
                std::fs::copy(&p, &target).unwrap();
            }
        }
    }

    /// Erledigt 4: Import einer Datei; fehlende Texte bleiben Englisch und
    /// werden gemeldet
    #[test]
    fn import_into_a_copy() {
        let src = root();
        let tmp = std::env::temp_dir().join(format!("micled-i18n-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for c in ["micled-core", "micled-ffi", "micled-cli"] {
            copy_dir(&src.join("crates").join(c).join("src"), &tmp.join("crates").join(c).join("src"), &|_| true);
        }
        copy_dir(&src.join(app::SOURCES), &tmp.join(app::SOURCES), &|_| true);
        copy_dir(&src.join("apps/MicLED/Resources"), &tmp.join("apps/MicLED/Resources"), &|p| p.extension().is_some_and(|x| x == "xcstrings"));

        let fixture = include_str!("../testdata/fr.po");
        let (code, out) = import(&tmp, fixture, None).unwrap();
        assert_eq!(code, "fr");
        let written = write(&tmp, &out).unwrap();
        assert!(written.contains(&"translations/fr.po".to_string()));
        assert!(written.contains(&LOCALES.to_string()));
        assert!(written.contains(&"apps/MicLED/Resources/Localizable.xcstrings".to_string()));

        let locales: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_str(&std::fs::read_to_string(tmp.join(LOCALES)).unwrap()).unwrap();
        assert_eq!(locales["fr"].get("Pixoo meldet Fehler {code}").map(String::as_str), Some("Le Pixoo signale l’erreur {code}"));
        assert!(!locales["fr"].contains_key("Pixoo nicht erreichbar: {e}"), "ungültige Platzhalter zählen als fehlend");

        let cat = J::parse(&std::fs::read_to_string(tmp.join("apps/MicLED/Resources/Localizable.xcstrings")).unwrap()).unwrap();
        let strings = cat.get("strings").unwrap();
        let translated = strings.entries().iter().filter_map(|(_, e)| e.get("localizations")?.get("fr")).filter(|l| l.get("stringUnit").and_then(|u| u.get("state")).and_then(J::as_str) == Some("translated")).count();
        assert!(translated >= 1);
        let fallback = strings.entries().iter().find_map(|(_, e)| e.get("localizations")?.get("fr").filter(|l| l.get("stringUnit").and_then(|u| u.get("state")).and_then(J::as_str) == Some("needs_review")));
        assert!(fallback.is_some(), "fehlende Texte stehen englisch mit needs_review");

        assert!(out.details.iter().any(|d| d.contains("Pixoo nicht erreichbar: {e}") && d.contains("invalid")));
        assert!(out.details.iter().any(|d| d.contains("missing")));
        assert!(out.summary[0].starts_with("fr: "));

        // zweiter Lauf ändert nichts mehr
        let again = export(&tmp, None).unwrap();
        assert!(write(&tmp, &again).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn import_needs_a_language() {
        let text = "msgid \"\"\nmsgstr \"\"\n\"Language: \\n\"\n";
        assert!(import(&root(), text, None).unwrap_err().contains("--lang"));
        assert!(import(&root(), text, Some("de")).is_err());
        assert!(import(&root(), "kaputt", Some("fr")).is_err());
    }
}
