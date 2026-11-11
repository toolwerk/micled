//! Gettext-PO lesen und schreiben, so weit MicLED es braucht: `msgctxt`,
//! Kommentare (`#`, `#.`, `#:`, `#,`, `#|`), mehrzeilige Strings und Escapes.
//! Plurale (`msgid_plural`) nutzt die Vorlage nicht; kommt eine Datei damit,
//! zählt `msgstr[0]`.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// `# …` vom Übersetzer, bleibt beim Nachführen erhalten
    pub translator: Vec<String>,
    /// `#. …` Hinweis aus der Vorlage
    pub extracted: Vec<String>,
    /// `#: …` Fundstellen
    pub refs: Vec<String>,
    /// `#, …` etwa `fuzzy`
    pub flags: Vec<String>,
    /// `#| msgid …` englischer Text, zu dem die Übersetzung passte
    pub prev_id: Option<String>,
    pub ctxt: Option<String>,
    pub id: String,
    pub text: String,
}

impl Entry {
    pub fn fuzzy(&self) -> bool {
        self.flags.iter().any(|f| f == "fuzzy")
    }

    pub fn set_fuzzy(&mut self, on: bool) {
        self.flags.retain(|f| f != "fuzzy");
        if on {
            self.flags.insert(0, "fuzzy".into());
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct File {
    /// Eintrag mit leerem `msgid` und ohne `msgctxt`
    pub header: Option<Entry>,
    pub entries: Vec<Entry>,
}

impl File {
    /// Wert einer Kopfzeile wie `Language`
    pub fn header_field(&self, name: &str) -> Option<String> {
        let h = self.header.as_ref()?;
        h.text.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim().eq_ignore_ascii_case(name)).then(|| v.trim().to_string())
        })
    }

    pub fn set_header_field(&mut self, name: &str, value: &str) {
        let h = self.header.get_or_insert_with(Entry::default);
        let line = format!("{name}: {value}\n");
        let mut found = false;
        let mut out = String::new();
        for l in h.text.split_inclusive('\n') {
            let is = l.split_once(':').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(name));
            if is && !found {
                out.push_str(&line);
                found = true;
            } else if !is {
                out.push_str(l);
            }
        }
        if !found {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&line);
        }
        h.text = out;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    None,
    Ctxt,
    Id,
    Plural,
    Str,
    StrOther,
    PrevId,
    PrevOther,
}

pub fn parse(text: &str) -> Result<File, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut file = File::default();
    let mut cur = Entry::default();
    let mut started = false; // msgid gesehen
    let mut has_str = false;
    let mut field = Field::None;
    let mut obsolete = false;

    let flush = |cur: &mut Entry, started: &mut bool, has_str: &mut bool, file: &mut File| {
        if *started {
            let e = std::mem::take(cur);
            if e.id.is_empty() && e.ctxt.is_none() && file.header.is_none() && file.entries.is_empty() {
                file.header = Some(e);
            } else {
                file.entries.push(e);
            }
        } else {
            *cur = Entry::default();
        }
        *started = false;
        *has_str = false;
    };

    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let err = |m: &str| format!("Zeile {}: {m}", n + 1);
        if line.is_empty() {
            if obsolete {
                obsolete = false;
                continue;
            }
            flush(&mut cur, &mut started, &mut has_str, &mut file);
            field = Field::None;
            continue;
        }
        if line.starts_with("#~") {
            if !started {
                cur = Entry::default(); // Kommentare davor gehören zum obsoleten Eintrag
            }
            obsolete = true;
            continue;
        }
        if obsolete {
            obsolete = false;
        }
        let new_part = line.starts_with('#') || line.starts_with("msgctxt") || (line.starts_with("msgid ") && started);
        if has_str && new_part {
            flush(&mut cur, &mut started, &mut has_str, &mut file);
        }
        if let Some(rest) = line.strip_prefix("#|") {
            let rest = rest.trim();
            if let Some(v) = rest.strip_prefix("msgid ") {
                cur.prev_id = Some(unquote(v).map_err(|m| err(&m))?);
                field = Field::PrevId;
            } else if rest.starts_with('"') {
                let v = unquote(rest).map_err(|m| err(&m))?;
                if field == Field::PrevId {
                    cur.prev_id.get_or_insert_with(String::new).push_str(&v);
                }
            } else {
                field = Field::PrevOther; // msgctxt oder msgid_plural davor
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#.") {
            cur.extracted.push(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("#:") {
            cur.refs.extend(rest.split_whitespace().map(str::to_string));
        } else if let Some(rest) = line.strip_prefix("#,") {
            cur.flags.extend(rest.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()));
        } else if let Some(rest) = line.strip_prefix('#') {
            cur.translator.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        } else if let Some(v) = line.strip_prefix("msgctxt ") {
            cur.ctxt = Some(unquote(v).map_err(|m| err(&m))?);
            field = Field::Ctxt;
        } else if let Some(v) = line.strip_prefix("msgid_plural ") {
            unquote(v).map_err(|m| err(&m))?;
            field = Field::Plural;
        } else if let Some(v) = line.strip_prefix("msgid ") {
            cur.id = unquote(v).map_err(|m| err(&m))?;
            started = true;
            field = Field::Id;
        } else if let Some(v) = line.strip_prefix("msgstr[0] ").or_else(|| line.strip_prefix("msgstr ")) {
            if !started {
                return Err(err("msgstr ohne msgid"));
            }
            cur.text = unquote(v).map_err(|m| err(&m))?;
            has_str = true;
            field = Field::Str;
        } else if line.starts_with("msgstr[") {
            has_str = true;
            field = Field::StrOther;
        } else if line.starts_with('"') {
            let v = unquote(line).map_err(|m| err(&m))?;
            match field {
                Field::Ctxt => cur.ctxt.get_or_insert_with(String::new).push_str(&v),
                Field::Id => cur.id.push_str(&v),
                Field::Str => cur.text.push_str(&v),
                Field::Plural | Field::StrOther | Field::PrevOther | Field::PrevId => {}
                Field::None => return Err(err("Text ohne Schlüsselwort")),
            }
        } else {
            return Err(err(&format!("unbekannte Zeile „{line}“")));
        }
    }
    flush(&mut cur, &mut started, &mut has_str, &mut file);
    Ok(file)
}

fn unquote(s: &str) -> Result<String, String> {
    let s = s.trim();
    let inner = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .filter(|_| s.len() >= 2)
        .ok_or_else(|| format!("Text nicht in Anführungszeichen: {s}"))?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            if c == '"' {
                return Err(format!("Anführungszeichen ohne \\ in {s}"));
            }
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('a') => out.push('\u{7}'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('v') => out.push('\u{b}'),
            Some(c @ ('\\' | '"' | '\'' | '?')) => out.push(c),
            Some(c) => return Err(format!("unbekanntes Escape \\{c} in {s}")),
            None => return Err(format!("\\ am Ende von {s}")),
        }
    }
    Ok(out)
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `msgid "…"`, mit Zeilenumbrüchen im Text als mehrere Zeilen
fn keyword(out: &mut String, prefix: &str, kw: &str, s: &str) {
    if s.contains('\n') {
        out.push_str(&format!("{prefix}{kw} \"\"\n"));
        for part in s.split_inclusive('\n') {
            out.push_str(&format!("{prefix}{}\n", quote(part)));
        }
    } else {
        out.push_str(&format!("{prefix}{kw} {}\n", quote(s)));
    }
}

fn write_entry(out: &mut String, e: &Entry) {
    for t in &e.translator {
        if t.is_empty() {
            out.push_str("#\n");
        } else {
            out.push_str(&format!("# {t}\n"));
        }
    }
    for x in &e.extracted {
        out.push_str(&format!("#. {x}\n"));
    }
    for r in &e.refs {
        out.push_str(&format!("#: {r}\n"));
    }
    if !e.flags.is_empty() {
        out.push_str(&format!("#, {}\n", e.flags.join(", ")));
    }
    if let Some(p) = &e.prev_id {
        keyword(out, "#| ", "msgid", p);
    }
    if let Some(c) = &e.ctxt {
        keyword(out, "", "msgctxt", c);
    }
    keyword(out, "", "msgid", &e.id);
    keyword(out, "", "msgstr", &e.text);
}

pub fn write(file: &File) -> String {
    let mut out = String::new();
    if let Some(h) = &file.header {
        write_entry(&mut out, h);
    }
    for e in &file.entries {
        if !out.is_empty() {
            out.push('\n');
        }
        write_entry(&mut out, e);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> File {
        File {
            header: Some(Entry { translator: vec!["MicLED".into()], text: "Language: fr\nContent-Type: text/plain; charset=UTF-8\n".into(), ..Default::default() }),
            entries: vec![
                Entry {
                    translator: vec!["vérifié".into(), String::new()],
                    extracted: vec!["Error message: Pixoo".into()],
                    refs: vec!["crates/micled-core/src/pixoo.rs".into()],
                    flags: vec!["fuzzy".into()],
                    prev_id: Some("Pixoo error {code}".into()),
                    ctxt: Some("Pixoo meldet Fehler {code}".into()),
                    id: "Pixoo reports error {code}".into(),
                    text: "Pixoo signale l'erreur {code}".into(),
                },
                Entry {
                    ctxt: Some("Localizable/Zeile\nzwei".into()),
                    id: "Line \"one\"\nand\ttwo \\ end\n".into(),
                    text: String::new(),
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn round_trip() {
        let f = sample();
        let text = write(&f);
        assert_eq!(parse(&text).unwrap(), f);
        assert_eq!(write(&parse(&text).unwrap()), text);
    }

    #[test]
    fn reads_poedit_wrapping_and_obsolete_entries() {
        let text = "msgid \"\"\nmsgstr \"\"\n\"Language: fr\\n\"\n\n#, fuzzy\n#| msgctxt \"alt\"\n#| msgid \"\"\n#| \"Old \"\n#| \"text\"\nmsgctxt \"A\"\nmsgid \"\"\n\"New \"\n\"text\"\nmsgstr \"\"\n\"Nouveau \"\n\"texte\"\n\n#~ msgid \"gone\"\n#~ msgstr \"parti\"\n";
        let f = parse(text).unwrap();
        assert_eq!(f.header_field("language").as_deref(), Some("fr"));
        assert_eq!(f.entries.len(), 1);
        let e = &f.entries[0];
        assert!(e.fuzzy());
        assert_eq!(e.prev_id.as_deref(), Some("Old text"));
        assert_eq!((e.ctxt.as_deref(), e.id.as_str(), e.text.as_str()), (Some("A"), "New text", "Nouveau texte"));
    }

    #[test]
    fn comments_of_obsolete_entries_stay_there() {
        let text = "msgid \"\"\nmsgstr \"Language: fr\"\n\n#, fuzzy\n#~ msgid \"old\"\n#~ msgstr \"vieux\"\n\nmsgctxt \"A\"\nmsgid \"a\"\nmsgstr \"b\"\n";
        let mut f = parse(text).unwrap();
        assert_eq!(f.entries.len(), 1);
        assert!(!f.entries[0].fuzzy());
        f.set_header_field("X-Test", "1");
        assert_eq!(f.header.unwrap().text, "Language: fr\nX-Test: 1\n");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("hello").is_err());
        assert!(parse("msgid \"a\nmsgstr \"\"").is_err());
        assert!(parse("msgid \"\\x\"\nmsgstr \"\"").is_err());
    }

    #[test]
    fn header_fields_are_replaced() {
        let mut f = sample();
        f.set_header_field("Language", "es");
        f.set_header_field("X-Test", "1");
        assert_eq!(f.header_field("Language").as_deref(), Some("es"));
        assert_eq!(f.header.unwrap().text, "Language: es\nContent-Type: text/plain; charset=UTF-8\nX-Test: 1\n");
    }
}
