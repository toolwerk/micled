//! Texte der App aus den String Catalogs (`*.xcstrings`): lesen für die
//! Vorlage, weitere Sprachen hineinschreiben.

use crate::json::J;
use regex::Regex;

/// Katalog-Name (Kontext in der Vorlage) und Pfad relativ zur Repo-Wurzel
pub const CATALOGS: [(&str, &str); 3] = [
    ("Localizable", "apps/MicLED/Resources/Localizable.xcstrings"),
    ("InfoPlist", "apps/MicLED/Resources/InfoPlist.xcstrings"),
    ("AppShortcuts", "apps/MicLED/Resources/AppShortcuts.xcstrings"),
];

pub const SOURCES: &str = "apps/MicLED/Sources";

/// Quell- und einzige mitgelieferte Übersetzungssprache
pub const BUILT_IN: [&str; 2] = ["de", "en"];

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    /// `<Katalog>/<Schlüssel>`, bei Pluralen mit `#one` usw.
    pub ctxt: String,
    pub en: String,
    pub hints: Vec<String>,
    pub refs: Vec<String>,
}

/// Wie ein Katalog-Eintrag übersetzt wird
enum Shape<'a> {
    Single(&'a str),
    /// Pluralformen des Englischen (`one`, `other` …)
    Plural(Vec<(&'a str, &'a str)>),
}

fn translatable(entry: &J) -> bool {
    !matches!(entry.get("shouldTranslate"), Some(J::Bool(false))) && entry.get("extractionState").and_then(J::as_str) != Some("stale")
}

fn shape<'a>(catalog: &str, key: &str, entry: &'a J) -> Result<Shape<'a>, String> {
    let en = entry
        .get("localizations")
        .and_then(|l| l.get("en"))
        .ok_or_else(|| format!("{catalog}: kein englischer Text für „{key}“"))?;
    if let Some(v) = en.get("stringUnit").and_then(|u| u.get("value")).and_then(J::as_str) {
        return Ok(Shape::Single(v));
    }
    let plural = en
        .get("variations")
        .and_then(|v| v.get("plural"))
        .ok_or_else(|| format!("{catalog}: englischer Text für „{key}“ ist weder Text noch Plural"))?;
    let forms = plural
        .entries()
        .iter()
        .map(|(cat, f)| {
            let v = f.get("stringUnit").and_then(|u| u.get("value")).and_then(J::as_str);
            v.map(|v| (cat.as_str(), v)).ok_or_else(|| format!("{catalog}: Pluralform {cat} für „{key}“ ohne Text"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Shape::Plural(forms))
}

/// Ort in der App zur Swift-Datei
fn place(file: &str) -> &'static str {
    match file.trim_end_matches(".swift") {
        "MenuView" => "Menu",
        "SettingsView" => "Settings window",
        "StatsView" => "Statistics window",
        "StatusContextMenu" => "Right-click menu",
        "StatusIcon" => "Menu bar icon",
        "Intents" => "Shortcuts app",
        _ => "App",
    }
}

/// Swift-Dateien, in denen der Schlüssel als String steht; `%lld`, `%@` …
/// und `${name}` (Kurzbefehle) im Schlüssel stehen dort als `\(…)`
fn find_in_swift(key: &str, swift: &[(String, String)]) -> Vec<String> {
    let spec = Regex::new(r"%(\d+\$)?[-+ #0]*\d*(\.\d+)?(hh|h|ll|l|q|z|t|j)?[@dDiuUxXoOfFeEgGcCsSpaA]|%%|\$\{\w+\}").expect("Regex");
    let mut pattern = String::from("\"");
    let mut last = 0;
    for m in spec.find_iter(key) {
        pattern.push_str(&regex::escape(&swift_literal(&key[last..m.start()])));
        pattern.push_str(if m.as_str() == "%%" { "%" } else { r"\\\(.+?\)" });
        last = m.end();
    }
    pattern.push_str(&regex::escape(&swift_literal(&key[last..])));
    pattern.push('"');
    let Ok(re) = Regex::new(&pattern) else { return Vec::new() };
    swift.iter().filter(|(_, src)| re.is_match(src)).map(|(name, _)| name.clone()).collect()
}

fn swift_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// Platzhalter, die eine Übersetzung behalten muss, für den Hinweis
pub fn keep_hint(en: &str) -> Option<String> {
    let p = crate::sync::placeholders(en);
    (!p.is_empty()).then(|| format!("Keep the placeholders: {}", p.join(" ")))
}

/// Alle zu übersetzenden Texte der Kataloge; `swift` sind Dateiname und
/// Inhalt der Swift-Quellen
pub fn texts(catalogs: &[(&str, J)], swift: &[(String, String)]) -> Result<Vec<Text>, String> {
    let mut out = Vec::new();
    for (name, cat) in catalogs {
        let strings = cat.get("strings").ok_or_else(|| format!("{name}: kein Feld strings"))?;
        for (key, entry) in strings.entries() {
            if !translatable(entry) {
                continue;
            }
            let (hint, refs) = match *name {
                "InfoPlist" => ("macOS permission prompt".to_string(), Vec::new()),
                "AppShortcuts" => ("Siri phrase of a shortcut".to_string(), Vec::new()),
                _ => {
                    let files = find_in_swift(key, swift);
                    let mut places: Vec<&str> = Vec::new();
                    for p in files.iter().map(|f| place(f)) {
                        if !places.contains(&p) {
                            places.push(p);
                        }
                    }
                    let comment = entry.get("comment").and_then(J::as_str).map(str::to_string);
                    let hint = comment.unwrap_or_else(|| if places.is_empty() { "App".to_string() } else { places.join(", ") });
                    (hint, files.iter().map(|f| format!("{SOURCES}/{f}")).collect())
                }
            };
            match shape(name, key, entry)? {
                Shape::Single(en) => {
                    let hints = std::iter::once(hint).chain(keep_hint(en)).collect();
                    out.push(Text { ctxt: format!("{name}/{key}"), en: en.to_string(), hints, refs });
                }
                Shape::Plural(forms) => {
                    for (cat, en) in forms {
                        let hints = [hint.clone(), format!("Plural form “{cat}”")].into_iter().chain(keep_hint(en)).collect();
                        out.push(Text { ctxt: format!("{name}/{key}#{cat}"), en: en.to_string(), hints, refs: refs.clone() });
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Übersetzung eines Kontexts, `None` heißt Englisch mit `needs_review`
pub type Lookup<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Schreibt die Sprachen `langs` (Code, Übersetzung je Kontext) in den
/// Katalog `name` und entfernt alle anderen außer Deutsch und Englisch
pub fn apply(name: &str, cat: &mut J, langs: &[(String, &Lookup)]) -> Result<(), String> {
    let Some(J::Obj(strings)) = cat.get_mut("strings") else {
        return Err(format!("{name}: kein Feld strings"));
    };
    for (key, entry) in strings.iter_mut() {
        if let Some(J::Obj(locs)) = entry.get_mut("localizations") {
            locs.retain(|(code, _)| BUILT_IN.contains(&code.as_str()));
        }
        if !translatable(entry) || langs.is_empty() {
            continue;
        }
        let add: Vec<(String, J)> = {
            let shape = shape(name, key, entry)?;
            langs
                .iter()
                .map(|(code, lookup)| {
                    let unit = |ctxt: String, en: &str| match lookup(&ctxt) {
                        Some(t) => string_unit("translated", &t),
                        None => string_unit("needs_review", en),
                    };
                    let loc = match &shape {
                        Shape::Single(en) => unit(format!("{name}/{key}"), en),
                        Shape::Plural(forms) => {
                            let forms = forms.iter().map(|(c, en)| (c.to_string(), unit(format!("{name}/{key}#{c}"), en))).collect();
                            J::obj([("variations", J::obj([("plural", J::Obj(forms))]))])
                        }
                    };
                    (code.clone(), loc)
                })
                .collect()
        };
        let Some(J::Obj(locs)) = entry.get_mut("localizations") else { continue };
        for (code, loc) in add {
            let at = locs.iter().position(|(c, _)| c.as_str() > code.as_str()).unwrap_or(locs.len());
            locs.insert(at, (code, loc));
        }
    }
    Ok(())
}

fn string_unit(state: &str, value: &str) -> J {
    J::obj([("stringUnit", J::obj([("state", J::str(state)), ("value", J::str(value))]))])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> J {
        J::parse(
            r#"{
  "sourceLanguage": "de",
  "strings": {
    "%lld": { "shouldTranslate": false },
    "%lld gefunden": { "localizations": {
      "de": { "stringUnit": { "state": "translated", "value": "%lld gefunden" } },
      "en": { "variations": { "plural": {
        "one": { "stringUnit": { "state": "translated", "value": "%lld found" } },
        "other": { "stringUnit": { "state": "translated", "value": "%lld found" } } } } },
      "zz": { "stringUnit": { "state": "translated", "value": "alt" } } } },
    "Hallo %@": { "localizations": {
      "de": { "stringUnit": { "state": "translated", "value": "Hallo %@" } },
      "en": { "stringUnit": { "state": "translated", "value": "Hello %@" } } } }
  },
  "version": "1.0"
}"#,
        )
        .unwrap()
    }

    #[test]
    fn reads_texts_with_place_and_plural() {
        let swift = vec![("MenuView.swift".to_string(), "Text(\"Hallo \\(name)\")".to_string())];
        let t = texts(&[("Localizable", catalog())], &swift).unwrap();
        let ctxts: Vec<_> = t.iter().map(|t| t.ctxt.as_str()).collect();
        assert_eq!(ctxts, ["Localizable/%lld gefunden#one", "Localizable/%lld gefunden#other", "Localizable/Hallo %@"]);
        assert_eq!(t[2].hints, ["Menu", "Keep the placeholders: %@"]);
        assert_eq!(t[2].refs, ["apps/MicLED/Sources/MenuView.swift"]);
        assert_eq!(t[0].hints[0], "App");
    }

    #[test]
    fn writes_language_and_removes_others() {
        let mut cat = catalog();
        let fr = |c: &str| (c == "Localizable/Hallo %@").then(|| "Bonjour %@".to_string());
        apply("Localizable", &mut cat, &[("fr".into(), &fr)]).unwrap();
        let locs = |k: &str| cat.get("strings").unwrap().get(k).unwrap().get("localizations").unwrap().clone();
        let hello = locs("Hallo %@");
        assert_eq!(hello.entries().iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["de", "en", "fr"]);
        assert_eq!(hello.get("fr"), Some(&string_unit("translated", "Bonjour %@")));
        let found = locs("%lld gefunden");
        assert_eq!(found.entries().iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["de", "en", "fr"]);
        let one = found.get("fr").unwrap().get("variations").unwrap().get("plural").unwrap().get("one").unwrap();
        assert_eq!(one, &string_unit("needs_review", "%lld found"));

        apply("Localizable", &mut cat, &[]).unwrap();
        assert_eq!(cat, {
            let mut c = catalog();
            apply("Localizable", &mut c, &[]).unwrap();
            c
        });
        assert!(cat.get("strings").unwrap().get("Hallo %@").unwrap().get("localizations").unwrap().get("fr").is_none());
    }
}
