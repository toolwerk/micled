# Changelog

Alle nennenswerten Änderungen an MicLED. Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen nach [SemVer](https://semver.org/lang/de/).

## [Unreleased]

### Hinzugefügt

- Rust-Kern: Luxafor per USB-HID (Gerät wird nur pro Befehl geöffnet) und Webhook, Mikrofon-Erkennung über CoreAudio-Prozessobjekte, Umschaltlogik mit Wartezeit, gemeinsame Statusdatei mit dem Stream-Deck-Plugin.
- Kommandozeile `micled` mit `set`, `off`, `mic` und `watch`.
- UniFFI-Schicht und SwiftUI-Menüleisten-App mit Schnellfarben, Farbwähler, Einstellungen, ignorierten Programmen und Start beim Anmelden.
- Bekannte Audio-Dienste, die das Mikrofon dauerhaft offen halten (`com.rogueamoeba.arkaudiod`), werden ab Werk ignoriert.
- Lampe geht bei Ruhezustand, ausgeschaltetem Bildschirm und Bildschirmsperre aus und bekommt danach ihre vorherige Farbe zurück. Während eines Calls bleibt die Mikrofarbe. Abschaltbar in den Einstellungen.
- Kamera-Erkennung über CoreMediaIO mit eigener Farbe.
- Eigene Farbe je Programm am Mikrofon.
- Konzentrations-Timer mit eigener Farbe und Blinken am Ende.
- Fokus von macOS als eigene Farbe, per `INFocusStatusCenter` oder Kurzbefehle-Automation.
- „Auf Sendung“-Meldung per HTTP-POST, z. B. an Home Assistant.
- Vorwarnung vor Kalenderterminen durch kurzes Blinken, Termine aus den Kalendern von macOS und optional direkt aus Nextcloud (CalDAV, App-Passwort im Schlüsselbund).
- Tagessumme „auf Sendung“ im Menü, gespeichert in `~/Library/Application Support/net.staude.micled/calls.json`.
- `micled://`-Adressen und Aktionen für die Kurzbefehle-App.
- `setup.sh`, `scripts/build-rust.sh`, `scripts/bump-build.sh`, `scripts/release.sh`, CI für Linux und macOS.
