# Changelog

Alle nennenswerten Änderungen an MicLED. Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen nach [SemVer](https://semver.org/lang/de/).

## [Unreleased]

### Hinzugefügt

- Divoom Pixoo 64 als Display: zeigt „ON AIR“ mit Mikro- oder Kamerasymbol, die Restminuten des Timers oder „FOKUS“ und kehrt danach zum vorherigen Kanal zurück. Einstellungen mit „Verbindung prüfen“, Kommandozeile `micled pixoo <adresse>`.
- Ulanzi-Pixeluhr mit AWTRIX 3: zeigt „ON AIR“ in der Mikro- oder Kamerafarbe, die Restminuten des Timers oder „FOKUS“ als Text bis zum Ende, danach wieder die normale Anzeige. Einstellungen mit „Verbindung prüfen“, Kommandozeile `micled awtrix <adresse>`.
- Philips Hue über die Hue Bridge: eine Lampe oder ein Raum zeigt die Farbe für Call, Timer und Fokus und bekommt danach An/Aus, Farbe und Helligkeit von vorher zurück. Koppeln per Knopf an der Bridge, App-Schlüssel im Schlüsselbund, Zertifikat der Bridge wird beim Koppeln festgehalten. Kommandozeile `micled hue pair` und `micled hue test`.
- Govee und Nanoleaf im eigenen Netz, je mehrere Geräte: Sie zeigen die Farbe für Call, Timer und Fokus und bekommen danach An/Aus, Helligkeit und Farbe von vorher zurück, Nanoleaf auch einen gespeicherten Effekt. Suche im eigenen Netz oder Adresse von Hand, Nanoleaf-Schlüssel im Schlüsselbund. Kommandozeile `micled govee scan|test` und `micled nanoleaf scan|pair|test`.
- Automatische Updates mit Sparkle: MicLED sucht täglich nach einer neuen Version und fragt vor der Installation nach. „Nach Updates suchen …“ im Menü und Kontextmenü, abschaltbar in den Einstellungen. `scripts/release.sh` signiert das ZIP und schreibt den Appcast.
- Lichtstreifen per USB mit Robobloq-Controller (`1A86:FE07`, z. B. Meewata): Der ganze Streifen zeigt die Farbe für Call, Timer und Fokus, sonst ist er aus. Erkennt das Wiederanstecken, Fehler im Menü bei fehlendem oder anderem Controller, „Testen“ in den Einstellungen lässt den Streifen etwa zwei Sekunden rot leuchten, zeigt danach wieder den aktuellen Zustand und nennt LEDs und Firmware. Kommandozeile `micled strip info|test`.
- „Keine Lampe“ in den Einstellungen: MicLED zeigt den Status dann nur auf dem Display, in Slack und am Webhook.
- Home Assistant per MQTT mit Discovery: MicLED erscheint als Gerät „MicLED (<Mac-Name>)“ mit den Sensoren Auf Sendung, Kamera und Programm, ist per Last Will nicht verfügbar, wenn MicLED endet oder der Mac schläft, und entfernt das Gerät beim Ausschalten wieder. Einstellungen mit Broker, Port, optional TLS, Benutzer und Passwort (im Schlüsselbund) und „Verbindung prüfen“, Kommandozeile `micled mqtt check`. Der Webhook bleibt als Alternative.

## [0.1.0] - 2026-11-05

### Hinzugefügt

- Rust-Kern: Luxafor per USB-HID (Gerät wird nur pro Befehl geöffnet) und Webhook, Mikrofon-Erkennung über CoreAudio-Prozessobjekte, Umschaltlogik mit Wartezeit, gemeinsame Statusdatei mit dem Stream-Deck-Plugin.
- Kommandozeile `micled` mit `set`, `off`, `mic` und `watch`.
- UniFFI-Schicht und SwiftUI-Menüleisten-App mit Schnellfarben, Farbwähler, Einstellungen, ignorierten Programmen und Start beim Anmelden.
- Bekannte Audio-Dienste, die das Mikrofon dauerhaft offen halten (`com.rogueamoeba.arkaudiod`), werden ab Werk ignoriert.
- Lampe geht bei Ruhezustand, ausgeschaltetem Bildschirm und Bildschirmsperre aus und bekommt danach ihre vorherige Farbe zurück. Während eines Calls bleibt die Mikrofarbe. Abschaltbar in den Einstellungen.
- Kamera-Erkennung über CoreMediaIO mit eigener Farbe.
- Eigene Farbe je Programm am Mikrofon.
- Konzentrations-Timer mit eigener Farbe und Blinken am Ende.
- Abends dimmen in einer einstellbaren Zeitspanne.
- Fenster „Statistik“ mit Balken je Tag und CSV-Export, Calls werden ein Jahr gespeichert.
- Fokus von macOS als eigene Farbe, per `INFocusStatusCenter` oder Kurzbefehle-Automation.
- „Auf Sendung“-Meldung per HTTP-POST, z. B. an Home Assistant.
- Slack-Status während eines Calls, vorheriger Status wird wiederhergestellt.
- Vorwarnung vor Kalenderterminen durch kurzes Blinken, Termine aus den Kalendern von macOS und optional direkt aus Nextcloud (CalDAV, App-Passwort im Schlüsselbund).
- Tagessumme „auf Sendung“ im Menü, gespeichert in `~/Library/Application Support/net.staude.micled/calls.json`.
- `micled://`-Adressen und Aktionen für die Kurzbefehle-App.
- `setup.sh`, `scripts/build-rust.sh`, `scripts/bump-build.sh`, `scripts/release.sh`, CI für Linux und macOS.

[Unreleased]: https://github.com/toolwerk/micled/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/toolwerk/micled/releases/tag/v0.1.0
