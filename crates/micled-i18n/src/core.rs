//! Texte des Rust-Kerns: jedes `tr!("deutsch", "english", name = wert)` in
//! `crates/*/src`, außer in Testcode.

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::path::Path;
use syn::parse::{ParseStream, Parser};
use syn::{Expr, Ident, LitStr, Token};

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub de: String,
    pub en: String,
    /// Pfad relativ zur Repo-Wurzel, mit `/`
    pub file: String,
    pub line: usize,
    /// Platzhalter in der Reihenfolge des englischen Textes
    pub names: Vec<String>,
}

/// Alle Texte aus `crates/*/src/**/*.rs`, sortiert nach Datei und Zeile
pub fn scan(root: &Path) -> Result<Vec<Text>, String> {
    let mut files = Vec::new();
    let crates = root.join("crates");
    for c in read_dir_sorted(&crates)? {
        if c.file_name().is_some_and(|n| n == "micled-i18n") {
            continue;
        }
        collect_rs(&c.join("src"), &mut files)?;
    }
    let mut out = Vec::new();
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
        let src = std::fs::read_to_string(&f).map_err(|e| format!("{rel}: {e}"))?;
        out.extend(scan_source(&rel, &src)?);
    }
    Ok(out)
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    v.sort();
    Ok(v)
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for p in read_dir_sorted(dir)? {
        if p.is_dir() {
            collect_rs(&p, out)?;
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    Ok(())
}

pub fn scan_source(file: &str, src: &str) -> Result<Vec<Text>, String> {
    let tokens: TokenStream = src.parse().map_err(|e| format!("{file}: {e}"))?;
    let mut out = Vec::new();
    walk(file, tokens, &mut out)?;
    Ok(out)
}

/// Sucht `tr ! ( … )`, auch in Klammern und anderen Makros, und überspringt
/// Elemente mit `#[cfg(test)]` oder `#[test]`
fn walk(file: &str, tokens: TokenStream, out: &mut Vec<Text>) -> Result<(), String> {
    let toks: Vec<TokenTree> = tokens.into_iter().collect();
    let mut i = 0;
    while i < toks.len() {
        if is_test_attr(&toks, i) {
            i = skip_item(&toks, i + 2);
            continue;
        }
        if let (TokenTree::Ident(id), Some(TokenTree::Punct(p)), Some(TokenTree::Group(g))) = (&toks[i], toks.get(i + 1), toks.get(i + 2)) {
            if id == "tr" && p.as_char() == '!' && g.delimiter() != Delimiter::None {
                out.push(parse_call(file, g.stream(), g.span().start().line)?);
                i += 3;
                continue;
            }
        }
        if let TokenTree::Group(g) = &toks[i] {
            walk(file, g.stream(), out)?;
        }
        i += 1;
    }
    Ok(())
}

fn is_test_attr(toks: &[TokenTree], i: usize) -> bool {
    let (Some(TokenTree::Punct(p)), Some(TokenTree::Group(g))) = (toks.get(i), toks.get(i + 1)) else {
        return false;
    };
    if p.as_char() != '#' || g.delimiter() != Delimiter::Bracket {
        return false;
    }
    let inner: String = g.stream().to_string().split_whitespace().collect();
    inner == "test" || inner == "cfg(test)"
}

/// Index hinter dem Element ab `i`: nach dem ersten `{ … }` oder `;`
/// außerhalb weiterer Attribute
fn skip_item(toks: &[TokenTree], mut i: usize) -> usize {
    while i < toks.len() {
        match &toks[i] {
            TokenTree::Punct(p) if p.as_char() == '#' => i += 2,
            TokenTree::Punct(p) if p.as_char() == ';' => return i + 1,
            TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => return i + 1,
            _ => i += 1,
        }
    }
    i
}

fn parse_call(file: &str, body: TokenStream, line: usize) -> Result<Text, String> {
    let at = format!("{file}:{line}");
    let parser = |input: ParseStream| -> syn::Result<(LitStr, LitStr, Vec<Ident>)> {
        let de: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let en: LitStr = input.parse()?;
        let mut names = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            input.parse::<Expr>()?;
            names.push(name);
        }
        Ok((de, en, names))
    };
    let (de, en, args) = parser.parse2(body).map_err(|e| format!("{at}: tr! nicht lesbar: {e}"))?;
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    let (de, en) = (de.value(), en.value());
    let de_names = placeholders(&de).map_err(|e| format!("{at}: {e}"))?;
    let en_names = placeholders(&en).map_err(|e| format!("{at}: {e}"))?;
    for n in de_names.iter().chain(&en_names) {
        if !args.contains(n) {
            return Err(format!("{at}: Platzhalter {{{n}}} ohne Argument, tr! braucht `{n} = …`"));
        }
    }
    Ok(Text { de, en, file: file.to_string(), line, names: en_names })
}

/// Namen der Platzhalter `{name}` in einem Text von `tr!`; positionale
/// Platzhalter und Formatangaben gehören ins Argument
pub fn placeholders(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find(['{', '}']) {
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('}') {
            return Err(format!("einzelne }} in „{s}“"));
        }
        let end = tail.find('}').ok_or_else(|| format!("offene {{ in „{s}“"))?;
        let name = &tail[1..end];
        let ident = !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ident {
            return Err(format!("Platzhalter {{{name}}} ohne Argument, tr! braucht benannte Platzhalter ohne Formatangabe"));
        }
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &tail[end + 1..];
    }
    Ok(out)
}

/// Gerät oder Bereich zur Datei, für den Hinweis in der Vorlage
pub fn area(file: &str) -> &'static str {
    let stem = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".rs");
    if file.starts_with("crates/micled-ffi/") {
        return "Settings check";
    }
    match stem {
        "awtrix" => "AWTRIX clock",
        "caldav" => "Nextcloud calendar",
        "camera" => "Camera detection",
        "color" => "Color",
        "govee" => "Govee",
        "hue" | "hue_net" => "Philips Hue",
        "lamp" => "Luxafor",
        "mic" => "Microphone detection",
        "monitor" => "Light strip",
        "mqtt" => "MQTT / Home Assistant",
        "nanoleaf" => "Nanoleaf",
        "nc_status" => "Nextcloud status",
        "onair" => "On-air webhook",
        "output" => "Device search",
        "pixoo" => "Pixoo",
        "robobloq" => "Light strip",
        "slack" => "Slack status",
        "wled" => "WLED",
        _ => "Error message",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_calls_with_named_placeholders() {
        let src = r#"
use crate::tr;
fn a(e: &str, n: &Dev) -> String {
    let x = Some(1).ok_or_else(|| crate::tr!("Pixoo {host} meldet {e}", "Pixoo {host} reports {e}", host = n.host, e = e));
    format!("{}", tr!(
        "Mehr{{zeilig}}",
        "Multi{{line}}",
    ))
}
#[cfg(test)]
mod tests {
    fn t() { tr!("Test", "Test"); }
}
#[test]
fn x() { tr!("Auch Test", "Also test"); }
macro_rules! tr { ($a:literal) => {} }
"#;
        let t = scan_source("crates/micled-core/src/pixoo.rs", src).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].de.as_str(), t[0].en.as_str(), t[0].line), ("Pixoo {host} meldet {e}", "Pixoo {host} reports {e}", 4));
        assert_eq!(t[0].names, ["host", "e"]);
        assert_eq!((t[1].de.as_str(), t[1].line), ("Mehr{{zeilig}}", 5));
    }

    #[test]
    fn placeholder_without_argument_names_file_and_line() {
        let src = "fn a() {\n    tr!(\"Fehler {e}\", \"Error {e}\");\n}\n";
        let err = scan_source("crates/x/src/a.rs", src).unwrap_err();
        assert!(err.starts_with("crates/x/src/a.rs:2:"), "{err}");
        assert!(err.contains("{e}"), "{err}");
        assert!(scan_source("a.rs", "fn a() { tr!(\"Fehler {}\", \"Error {}\", x); }").is_err());
        assert!(scan_source("a.rs", "fn a() { tr!(\"Fehler {e:?}\", \"Error {e:?}\", e = e); }").is_err());
    }

    #[test]
    fn reads_every_core_text() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let texts = scan(&root).unwrap();
        assert!(texts.len() > 50, "{}", texts.len());
        assert!(texts.iter().all(|t| !t.file.contains("lang.rs") || !t.de.contains("Fehler {code}")));
        assert!(texts.iter().any(|t| t.file == "crates/micled-ffi/src/lib.rs"));
        assert!(!texts.iter().any(|t| t.de == "FOKUS"), "tr_fixed! bleibt außen vor");
    }
}
