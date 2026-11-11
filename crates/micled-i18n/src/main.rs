//! Übersetzungen für MicLED, aufgerufen über `scripts/translations.sh`:
//!
//! - `export` (Standard): schreibt `translations/template.pot` aus allen Texten
//!   von App und Kern und führt jede `translations/<code>.po` nach, dazu
//!   `locales.json` des Kerns und die String Catalogs der App.
//! - `import <datei.po> [--lang <code>]`: übernimmt eine ausgefüllte Datei als
//!   `translations/<code>.po` und meldet, was fehlt.

mod app;
mod core;
mod json;
mod po;
mod sync;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: translations.sh [export] | import <file.po> [--lang <code>]";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut root = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut lang = None;
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = PathBuf::from(it.next().ok_or(USAGE)?),
            "--lang" => lang = Some(it.next().ok_or(USAGE)?),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => rest.push(a),
        }
    }
    match rest.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] | ["export"] if lang.is_none() => {
            let out = sync::export(&root, None)?;
            report(&sync::write(&root, &out)?);
            for s in &out.summary {
                println!("{s}");
            }
            if out.summary.is_empty() {
                println!("no translations yet, template has the current texts");
            }
            Ok(())
        }
        ["import", file] => {
            let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
            let (code, out) = sync::import(&root, &text, lang.as_deref()).map_err(|e| format!("{file}: {e}"))?;
            report(&sync::write(&root, &out)?);
            for d in &out.details {
                println!("{d}");
            }
            for s in &out.summary {
                println!("{s}");
            }
            println!("saved as translations/{code}.po; missing texts show in English");
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn report(written: &[String]) {
    for w in written {
        println!("updated {w}");
    }
}
