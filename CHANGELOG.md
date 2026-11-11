# Changelog

Alle nennenswerten Änderungen an MicLED. Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionen nach [SemVer](https://semver.org/lang/de/).

## [Unreleased]

### Hinzugefügt

- Divoom Pixoo 64 als Display: zeigt „ON AIR“ mit Mikro- oder Kamerasymbol, die Restminuten des Timers oder „FOKUS“ und kehrt danach zum vorherigen Kanal zurück. Einstellungen mit „Verbindung prüfen“, Kommandozeile `micled pixoo <adresse>`.
- Ulanzi-Pixeluhr mit AWTRIX 3: zeigt „ON AIR“ in der Mikro- oder Kamerafarbe, die Restminuten des Timers oder „FOKUS“ als Text bis zum Ende, danach wieder die normale Anzeige. Einstellungen mit „Verbindung prüfen“, Kommandozeile `micled awtrix <adresse>`.
- Philips Hue über die Hue Bridge: eine Lampe oder ein Raum zeigt die Farbe für Call, Timer und Fokus und bekommt danach An/Aus, Farbe und Helligkeit von vorher zurück. Koppeln per Knopf an der Bridge, App-Schlüssel im Schlüsselbund, Zertifikat der Bridge wird beim Koppeln festgehalten. Kommandozeile `micled hue pair` und `micled hue test`.
- Govee und Nanoleaf im eigenen Netz, je mehrere Geräte: Sie zeigen die Farbe für Call, Timer und Fokus und bekommen danach An/Aus, Helligkeit und Farbe von vorher zurück, Nanoleaf auch einen gespeicherten Effekt. Suche im eigenen Netz oder Adresse von Hand, Nanoleaf-Schlüssel im Schlüsselbund. Kommandozeile `micled govee scan|test` und `micled nanoleaf scan|pair|test`.
- WLED im eigenen Netz, mehrere Geräte: Alle Segmente zeigen die Farbe für Call, Timer und Fokus und bekommen danach An/Aus, Helligkeit und je Segment Farbe und Effekt von vorher zurück; eine Playlist startet neu, Live-Daten anderer Steuerungen haben während des Calls Nachrang, ein laufendes Nachtlicht endet, Befehle gehen nicht per WLED-Sync an andere Geräte. Suche per Bonjour oder Adresse von Hand, ohne Koppeln. Kommandozeile `micled wled scan|test`.
- Automatische Updates mit Sparkle: MicLED sucht täglich nach einer neuen Version und fragt vor der Installation nach. „Nach Updates suchen …“ im Menü und Kontextmenü, abschaltbar in den Einstellungen. `scripts/release.sh` signiert das ZIP und schreibt den Appcast.
- Lichtstreifen per USB mit Robobloq-Controller (`1A86:FE07`, z. B. Meewata): Der ganze Streifen zeigt die Farbe für Call, Timer und Fokus, sonst ist er aus. Erkennt das Wiederanstecken, Fehler im Menü bei fehlendem oder anderem Controller, „Testen“ in den Einstellungen lässt den Streifen etwa zwei Sekunden rot leuchten, zeigt danach wieder den aktuellen Zustand und nennt LEDs und Firmware. Kommandozeile `micled strip info|test`.
- „Keine Lampe“ in den Einstellungen: MicLED zeigt den Status dann nur auf dem Display, in Slack, in Nextcloud und am Webhook.
- Ende des Termins auf Pixoo 64 und AWTRIX-Uhr: Läuft ein Call während eines Termins, zeigt das Pixoo unter „ON AIR“ „BIS 14:30“, die AWTRIX-Uhr „ON AIR BIS 14:30“ als Lauftext, nie den Titel. Ganztägige, abgesagte und (im Kalender von macOS) abgelehnte Termine zählen nicht, nach dem Terminende verschwindet die Zeit. Abschaltbar unter Einstellungen › Termine. Die Kalender werden dafür auch ohne Vorwarnung gelesen, sobald Pixoo oder AWTRIX an ist.
- Nextcloud-Status: Während du auf Sendung bist, steht im Benutzerstatus z. B. 🔴 „Im Call“, auf Wunsch mit „Nicht stören“; danach kommt der vorherige Status zurück. Eine vorhandene Nachricht überschreibt MicLED nicht, Fehler stehen im Menü.
- Zustände je Gerät: Pixoo 64, AWTRIX, Hue, Govee, Nanoleaf, WLED und der Lichtstreifen haben in den Einstellungen die Häkchen „Call“, „Timer“ und „Fokus“. Ein abgewählter Zustand gilt für das Gerät als frei, es zeigt dann den nächsten gewählten darunter oder seinen Zustand von vorher. Ab Werk und nach dem Update sind alle gesetzt.

### Geändert

- Der Nextcloud-Zugang steht in einem eigenen Abschnitt „Nextcloud“ mit den Schaltern „Termine lesen“ (bisher „Nextcloud direkt“ unter Termine) und „Status setzen“. Wer „Nextcloud direkt“ an hatte, behält Zugang und Vorwarnung.

### Behoben

- Abgesagte Termine im Kalender von macOS lösen keine Vorwarnung mehr aus.

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
