# MicLED

[![macOS 14+](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](#voraussetzungen)
[![Swift 6](https://img.shields.io/badge/Swift-6-F05138?logo=swift&logoColor=white)](apps/MicLED)
[![Rust](https://img.shields.io/badge/Rust-Kern-B7410E?logo=rust&logoColor=white)](crates/micled-core)
[![CI](https://github.com/toolwerk/micled/actions/workflows/ci.yml/badge.svg)](https://github.com/toolwerk/micled/actions/workflows/ci.yml)
[![Lizenz: MIT](https://img.shields.io/badge/Lizenz-MIT-blue)](LICENSE)

**Deine Luxafor-Lampe zeigt, wann du auf Sendung bist.** MicLED sitzt in der macOS-Menüleiste und schaltet die Lampe auf Rot, sobald ein Programm das Mikrofon oder die Kamera nutzt. Danach kommt die Farbe von vorher zurück.

<p align="center">
  <img src="docs/images/lamp.gif" alt="Luxafor Flag am Monitor: grün, beim Call rot, danach wieder grün" width="340">
  &nbsp;&nbsp;
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/menu-dark.png">
    <img src="docs/images/menu-light.png" alt="Menü von MicLED: Mikrofon aktiv in Zoom, Schnellfarben, Konzentrations-Timer und Zeit auf Sendung" width="300">
  </picture>
</p>

## Funktionen

- 🎙️ **Mikrofon und Kamera** werden erkannt, egal welches Programm: Zoom, Teams, Meet im Browser, FaceTime oder Plaud. Ton wird dabei nicht gelesen.
- 🎨 **Farben nach Wunsch**: eigene Farbe für Mikrofon, Kamera und einzelne Programme, danach zurück zur vorherigen Farbe.
- ⏱️ **Konzentrations-Timer** mit eigener Farbe; am Ende blinkt die Lampe.
- 🌙 **Fokus und Abend**: eigene Farbe, solange ein Fokus von macOS an ist, abends gedimmt.
- 📅 **Vorwarnung vor Terminen** aus den Kalendern von macOS oder direkt aus Nextcloud.
- 💬 **Slack-Status** „Im Call“, solange du auf Sendung bist.
- 🏠 **Home Assistant**: „Auf Sendung“ per Webhook, z. B. für ein Türschild.
- 📊 **Statistik**: Zeit auf Sendung je Tag, Export als CSV.
- ⌨️ **Steuerung von außen** über `micled://`-Adressen, Kurzbefehle und das Stream Deck.
- 🔌 **USB direkt** ohne Luxafor-App, wahlweise über den Luxafor-Webhook.

## Screenshots

<table>
  <tr>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/images/settings-dark.png">
        <img src="docs/images/settings-light.png" alt="Einstellungen von MicLED">
      </picture>
    </td>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/images/stats-dark.png">
        <img src="docs/images/stats-light.png" alt="Statistik: Minuten auf Sendung je Tag als Balken">
      </picture>
    </td>
  </tr>
  <tr>
    <td align="center">Einstellungen</td>
    <td align="center">Statistik</td>
  </tr>
</table>

<details>
<summary><b>Alle Funktionen im Detail</b></summary>

- Erkennt jedes Programm, das gerade vom Mikrofon liest (Zoom, Teams, Meet im Browser, Plaud, FaceTime …), über CoreAudio. Ton wird dabei nicht gelesen, eine Mikrofon-Freigabe ist nicht nötig.
- Zwei einstellbare Farben: „Mikro an“ (Standard Rot) und „Mikro aus“ (Standard Grün).
- Nach dem Mikrofon wahlweise zurück zur vorherigen Farbe oder immer auf die „Mikro aus“-Farbe.
- Schnellfarben und Farbwähler im Menü zum direkten Umschalten.
- Ansteuerung per USB direkt (ohne Luxafor-App) oder per Luxafor-Webhook.
- Programme lassen sich ignorieren, etwa ein Diktierwerkzeug, das das Mikrofon dauerhaft offen hält. Bekannte Audio-Dienste wie `arkaudiod` von Rogue Amoeba sind ab Werk eingetragen.
- Bei Ruhezustand, ausgeschaltetem Bildschirm und Sperre geht die Lampe aus und danach mit der vorherigen Farbe wieder an. Während eines Calls bleibt sie in der Mikrofarbe.
- Wartezeit vor dem Zurückschalten (Standard 3 Sekunden), damit kurze Pausen nicht flackern.
- Kamera-Erkennung über CoreMediaIO: Kamera an zählt wie Mikro an und hat eine eigene Farbe (Standard Magenta).
- Farbe je Programm, z. B. Orange für eine Aufnahme mit Plaud und Rot für Calls. Bei mehreren Programmen gilt der oberste Eintrag, die Kamerafarbe hat Vorrang.
- „Auf Sendung“ nach außen melden: Bei jedem Wechsel geht ein POST mit JSON an eine Adresse, etwa einen Home-Assistant-Webhook für ein Türschild.
- Slack-Status: Während du auf Sendung bist, steht z. B. 🔴 „Im Call“ in deinem Slack-Profil, danach kommt der vorherige Status zurück.
- Vorwarnung vor Terminen: Die Lampe blinkt kurz vor jedem Termin (Vorlauf einstellbar, ab Werk aus). Termine kommen aus den Kalendern von macOS und auf Wunsch direkt aus Nextcloud.
- Konzentrations-Timer: Die Lampe zeigt 15 bis 90 Minuten lang eine eigene Farbe und blinkt am Ende. Start im Menü, per Rechtsklick auf das Symbol, über `micled://timer/25` oder Kurzbefehle.
- Fokus von macOS: Solange ein Fokus an ist, leuchtet die Lampe in der Fokusfarbe. Gibt macOS den Fokus nicht frei, meldet ihn eine Kurzbefehle-Automation („Fokus an MicLED melden“).
- Abends dimmen: In einer einstellbaren Zeitspanne (Standard 18–7 Uhr) leuchtet die Lampe gedimmt, die Farben rechnet MicLED herunter.
- Statistik: Im Menü steht, wie lange du heute auf Sendung warst. Das Fenster „Statistik“ zeigt die letzten 7 oder 30 Tage als Balken und exportiert alle Calls als CSV. Gespeichert wird ein Jahr.
- Steuerung von außen über `micled://`-Adressen und Aktionen in der Kurzbefehle-App, siehe unten.

</details>

## Steuerung von außen

Adressen, z. B. für Raycast, das Stream Deck („Website öffnen“) oder `open` im Terminal:

```text
micled://color/ff8800      Farbe setzen (auch micled://farbe/rot)
micled://off               Lampe aus
micled://auto/on           Automatik an (auch off, toggle)
micled://timer/25          Konzentrations-Timer für 25 Minuten (timer/off beendet ihn)
micled://focus/on          Fokus melden (auch off), z. B. aus einer Fokus-Automation
```

In der Kurzbefehle-App gibt es die Aktionen „Lampenfarbe setzen“, „Lampe ausschalten“, „Mikrofon-Automatik schalten“, „Konzentration starten“, „Konzentration beenden“ und „Fokus an MicLED melden“. Gibt macOS den Fokus nicht frei, meldet ihn eine Automation: Kurzbefehle › Automation › Fokus „Arbeiten“ an › „Fokus an MicLED melden“ (und eine zweite für aus).

## „Auf Sendung“ an Home Assistant

In den Einstellungen die Webhook-Adresse eintragen, z. B. `http://homeassistant.local:8123/api/webhook/micled`. MicLED sendet bei jedem Wechsel:

```json
{"on_air": true, "camera": false, "apps": ["us.zoom.xos"]}
```

In einer Home-Assistant-Automation mit Webhook-Auslöser steht der Wert als `{{ trigger.json.on_air }}` bereit.

## Slack-Status

1. Unter <https://api.slack.com/apps> › „Create New App“ › „From scratch“ eine App im eigenen Workspace anlegen.
2. „OAuth & Permissions“ › „User Token Scopes“: `users.profile:read` und `users.profile:write` hinzufügen.
3. „Install to Workspace“ und den „User OAuth Token“ (`xoxp-…`) kopieren.
4. In MicLED unter Einstellungen › Slack einfügen und „Verbindung prüfen“ drücken.

MicLED setzt den Status nur, wenn gerade keiner gesetzt ist. Ändert sich der Status während des Calls, bleibt er danach stehen. Der eigene Status läuft nach vier Stunden von selbst ab, falls MicLED ihn nicht mehr zurücksetzen kann.

## Termine aus Nextcloud

Für die Vorwarnung gibt es zwei Wege:

1. **Über macOS** (empfohlen, wenn du den Kalender ohnehin auf dem Mac nutzt): Systemeinstellungen › Internet-Accounts › Account hinzufügen › Andere › CalDAV-Account, Accounttyp „Manuell“, Server `https://<deine-nextcloud>/remote.php/dav`. MicLED liest alle Kalender, die in der Kalender-App erscheinen.
2. **Direkt in MicLED**: Einstellungen › Termine › „Nextcloud direkt“ einschalten, Server (z. B. `https://cloud.example.org`), Benutzer und ein App-Passwort eintragen. Das App-Passwort legst du in Nextcloud unter Einstellungen › Sicherheit › „Neues App-Passwort erstellen“ an. MicLED speichert es im Schlüsselbund und fragt alle zwei Minuten die Termine der nächsten Stunde ab, Serientermine eingeschlossen. „Verbindung prüfen“ zeigt, wie viele Kalender gefunden wurden.

Beide Wege lassen sich kombinieren, ein Termin in beiden Quellen blinkt nur einmal. Wer nur Nextcloud direkt nutzt, schaltet „Kalender von macOS“ aus; dann fragt MicLED auch nicht nach dem Kalenderzugriff. Beim direkten Abruf wird nicht ausgewertet, ob du einen Termin abgelehnt hast.

## Woher kennt MicLED die vorherige Farbe?

Die Lampe kann ihre aktuelle Farbe nicht melden, weder per USB noch per Webhook. MicLED merkt sich deshalb die Farbe, die zuletzt gesetzt wurde:

- über das eigene Menü,
- über das Stream-Deck-Plugin `net.staude.luxafor` ab Version 1.1. Es schreibt jede feste Farbe in die gemeinsame Datei `~/Library/Application Support/net.staude.luxafor/state.json`.

Farbwechsel aus der Luxafor-App bekommt MicLED nicht mit.

## Voraussetzungen

- macOS 14 oder neuer
- Xcode (wird nur zum Bauen gebraucht, nicht geöffnet), `xcodegen`, Rust über `rustup`
- Luxafor Flag, Orb oder Bluetooth (USB-Kennung 04d8:f372)

## Einrichten

```bash
./setup.sh            # Werkzeuge prüfen, Tests, Rust-Kern bauen, Xcode-Projekt erzeugen
./setup.sh --github   # zusätzlich privates Repo toolwerk/micled anlegen und pushen
open apps/MicLED/MicLED.xcodeproj
```

Vor dem ersten signierten Build die Team-ID in `apps/MicLED/project.yml` unter `DEVELOPMENT_TEAM` eintragen.

## Kommandozeile

Der Rust-Kern lässt sich ohne Xcode testen:

```bash
cargo run -p micled-cli -- mic                 # wer nutzt gerade das Mikrofon, läuft eine Kamera?
cargo run -p micled-cli -- set ff8800          # Farbe setzen (auch: rot, gruen, aus)
cargo run -p micled-cli -- off
cargo run -p micled-cli -- watch               # Automatik, Strg-C beendet
cargo run -p micled-cli -- watch --on ff0000 --off 00ff00 --fixed --delay 5
cargo run -p micled-cli -- watch --camera ff00ff --on-air-url http://…   # oder --no-camera
cargo run -p micled-cli -- --webhook <ID> set gruen
MICLED_NEXTCLOUD_PASSWORD=<app-passwort> cargo run -p micled-cli -- nextcloud https://cloud.example.org <benutzer>
```

## Aufbau

```
crates/micled-core   Lampe (USB/Webhook), Mikrofon und Kamera, Logik, Statusdatei, Nextcloud-Kalender
crates/micled-cli    Kommandozeile `micled`
crates/micled-ffi    UniFFI-Schicht für Swift
apps/MicLED          SwiftUI-Menüleisten-App, project.yml für xcodegen
scripts/             build-rust.sh, bump-build.sh, release.sh, screenshots.sh
docs/images/         Screenshots und Lampen-Video (lamp.gif) für diese README
```

Das Xcode-Target `RustCore` (Abhängigkeit der App) ruft `scripts/build-rust.sh` auf. Das Skript baut eine universelle statische Bibliothek, erzeugt die Swift-Bindings und legt Bibliothek und Header unter `build/universal` und `build/include` ab. Generierte Dateien werden nicht eingecheckt.

## Screenshots aktualisieren

```bash
scripts/screenshots.sh
```

Das Skript baut die Debug-App, startet sie mit Beispieldaten (Schalter `--screenshots`, nur in Debug-Builds) und nimmt Menü, Einstellungen und Statistik in Hell und Dunkel nach `docs/images/` auf. Lampe, Slack und Nextcloud bleiben dabei unberührt. Das Terminal braucht einmalig das Recht „Bildschirmaufnahme“ (Systemeinstellungen › Datenschutz & Sicherheit).

## Release

```bash
xcrun notarytool store-credentials micled --apple-id <Apple-ID> --team-id <TEAM> --password <app-spezifisch>
scripts/bump-build.sh
scripts/release.sh
```

`release.sh` prüft zuerst Zertifikat, Notarisierungsprofil, Team-ID und sauberen Git-Stand. Danach folgen Archiv, Export mit Developer ID, Notarisierung, Stapeln und Prüfung mit `codesign`, `spctl` und `stapler`.

## Lizenz

MIT
