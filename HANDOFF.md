# Übergabe an lokales Claude Code (2026-11-04)

Entstanden in einer Cloud-Session ohne Xcode. Rust-Teil ist getestet, Swift-Teil nie kompiliert.

## Stand

- `cargo test --workspace`: 14 Tests grün, `cargo clippy -D warnings` sauber (unter Linux).
- `mic.rs` (CoreAudio-Prozessobjekte) für `aarch64-apple-darwin` typgeprüft, aber nie gegen echtes CoreAudio gelaufen.
- Swift-Aufrufe gegen die von UniFFI 0.32 erzeugten Bindings abgeglichen (Namen und Signaturen passen).
- Webhook und Einlesen der Statusdatei des Stream-Deck-Plugins per CLI durchgespielt.

## Nächste Schritte

1. Git-Repo initialisieren, erster Commit (`./setup.sh` setzt Identität `frank@staude.net`).
2. `cargo run -p micled-cli -- mic` mit laufendem Zoom/Meet/Plaud: liefert die Prozessliste etwas? Falls OSStatus-Fehler: FourCCs in `crates/micled-core/src/mic.rs` prüfen (`prs#`, `ppid`, `pbid`, `piri`).
3. `cargo run -p micled-cli -- watch` mit angesteckter Lampe.
4. Team-ID in `apps/MicLED/project.yml` eintragen, `./setup.sh`, in Xcode bauen. Erwartbare Baustellen: Swift-6-Concurrency-Meldungen (Observer, `Task { @MainActor }`), Linker-Frameworks für die Rust-Bibliothek (in `project.yml` unter `dependencies`).
5. Danach `./setup.sh --github` für `staude/micled`, Issues für offene Punkte anlegen.

## Wichtige Befunde

- Luxafor kann die aktuelle Farbe nicht melden. Daher Statusdatei `~/Library/Application Support/net.staude.luxafor/state.json`, geteilt mit dem Stream-Deck-Plugin.
- USB-Gerät nur pro Befehl öffnen, sonst `DeviceNotFound` in anderen Programmen.
- Vor jeder Farbe `00 4F` senden und Farbe doppelt schreiben, sonst bleibt ein laufendes Muster stehen.
- Crate `luxafor` 0.3.0 nicht verwenden: Hex-Parsing fehlerhaft, `off` wirkungslos.

Vault-Notiz: `02 Projekte/MicLED.md`. Projektregeln: `CLAUDE.md`.
