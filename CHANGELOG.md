# Changelog

Alle nennenswerten Änderungen an MicLED. Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen nach [SemVer](https://semver.org/lang/de/).

## [Unreleased]

### Hinzugefügt

- Rust-Kern: Luxafor per USB-HID (Gerät wird nur pro Befehl geöffnet) und Webhook, Mikrofon-Erkennung über CoreAudio-Prozessobjekte, Umschaltlogik mit Wartezeit, gemeinsame Statusdatei mit dem Stream-Deck-Plugin.
- Kommandozeile `micled` mit `set`, `off`, `mic` und `watch`.
- UniFFI-Schicht und SwiftUI-Menüleisten-App mit Schnellfarben, Farbwähler, Einstellungen, ignorierten Programmen und Start beim Anmelden.
- Bekannte Audio-Dienste, die das Mikrofon dauerhaft offen halten (`com.rogueamoeba.arkaudiod`), werden ab Werk ignoriert.
- `setup.sh`, `scripts/build-rust.sh`, `scripts/bump-build.sh`, `scripts/release.sh`, CI für Linux und macOS.
