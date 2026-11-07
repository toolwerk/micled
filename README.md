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
- 🖼️ **Pixoo 64**: „ON AIR“, Restzeit des Timers oder Fokus auf einem Divoom-Display im eigenen Netz, auch ganz ohne Lampe.
- 💡 **Philips Hue**: eine Hue-Lampe oder ein ganzer Raum leuchtet in der Statusfarbe und kehrt danach zum Licht von vorher zurück.
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

## Pixoo 64

Ein Divoom Pixoo 64 im selben WLAN zeigt den Status zusätzlich zur Lampe oder statt ihr.

<p align="center">
  <img src="docs/images/pixoo.gif" alt="Pixoo 64 im Regal: zeigt die Uhr, beim Call ein rotes Mikrofon mit „ON AIR“, danach wieder die Uhr" width="300">
</p>

Es zeigt:

- auf Sendung: Mikro- oder Kamerasymbol mit „ON AIR“ in der Mikro-, Kamera- oder Programmfarbe,
- Konzentrations-Timer: die restlichen Minuten mit „MIN“, jede Minute neu,
- Fokus: „FOKUS“ in der Fokusfarbe.

Danach, beim Ausschalten und beim Beenden von MicLED läuft auf dem Display wieder, was vorher lief, z. B. die Uhr. Abends gedimmt werden auch die Farben auf dem Display.

1. In der Divoom-App die IP-Adresse des Pixoo nachsehen (Gerät › Einstellungen › WLAN) und ihm im Router eine **feste IP-Adresse** geben. Sonst findet MicLED das Display nach einem Neustart des Routers nicht mehr.
2. In MicLED unter Einstellungen › Pixoo 64 „Status auf dem Pixoo 64 zeigen“ einschalten, die Adresse eintragen und „Verbindung prüfen“ drücken.
3. macOS fragt beim ersten Mal nach dem Zugriff auf das **lokale Netzwerk**. Ohne diese Erlaubnis ist das Pixoo nicht erreichbar (ebenso ein Home Assistant im eigenen Netz). Nachträglich: Systemeinstellungen › Datenschutz & Sicherheit › Lokales Netzwerk › MicLED.

Ist das Display nicht erreichbar, steht ein Fehler im Menü und MicLED versucht es alle 30 Sekunden erneut. Die Lampe schaltet trotzdem sofort.

## Philips Hue

Eine Hue-Lampe, etwa vor der Tür, oder alle Lampen eines Raums zeigen den Status zusätzlich zur Lampe oder statt ihr. MicLED spricht dafür direkt mit der Hue Bridge im eigenen Netz, ohne Cloud und ohne Hue-Konto.

Die Lampe leuchtet auf Sendung in der Mikro-, Kamera- oder Programmfarbe, beim Konzentrations-Timer und bei Fokus in deren Farbe, abends entsprechend gedimmt. Danach, beim Ausschalten und beim Beenden von MicLED hat jede Lampe wieder An/Aus, Farbe und Helligkeit von vorher. Eine vorher ausgeschaltete Lampe leuchtet dabei kurz in ihrer alten Farbe auf, bevor sie ausgeht; so hat sie beim nächsten Einschalten nicht die Call-Farbe. Was du während eines Calls in der Hue-App änderst, überschreibt MicLED danach mit dem Zustand von vor dem Call.

1. In der Hue-App die IP-Adresse der Bridge nachsehen (Einstellungen › Bridges › Bridge › IP-Adresse) und ihr im Router eine **feste IP-Adresse** geben.
2. In MicLED unter Einstellungen › Philips Hue „Status auf einer Hue-Lampe zeigen“ einschalten und die Adresse eintragen.
3. Den runden Knopf auf der Bridge drücken und innerhalb von 30 Sekunden „Koppeln“ wählen. Es erscheint „Gekoppelt ✓“. Der App-Schlüssel liegt im Schlüsselbund.
4. Unter „Lampe“ einen Raum („Raum: …“) oder eine einzelne Lampe wählen. Bei einem Raum werden alle Lampen darin geschaltet; das Zurückstellen dauert bei vielen Lampen ein bis zwei Sekunden.

Beim Koppeln merkt sich MicLED das Zertifikat der Bridge und spricht danach nur mit genau dieser. Wird die Bridge ausgetauscht oder zurückgesetzt, steht im Menü „Zertifikat der Bridge hat sich geändert, bitte neu koppeln“; dann Schritt 3 wiederholen. Eine neue Adresse löscht die Kopplung ebenfalls.

Ist die Bridge nicht erreichbar, steht ein Fehler im Menü und MicLED versucht es alle 30 Sekunden erneut. Lampe und Pixoo schalten trotzdem sofort. Wie beim Pixoo braucht MicLED den Zugriff auf das **lokale Netzwerk**.

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
open apps/MicLED/MicLED.xcodeproj
```

Vor dem ersten signierten Build die Team-ID in `apps/MicLED/project.yml` unter `DEVELOPMENT_TEAM` eintragen.

Unter Einstellungen › Verbindung zur Lampe wählst du „USB direkt“, „Webhook“ (über die Luxafor-App) oder **„Keine Lampe“**. Ohne Lampe zeigt MicLED den Status nur auf dem [Pixoo 64](#pixoo-64), in Slack und am Webhook, und im Menü steht kein Lampenfehler.

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
cargo run -p micled-cli -- pixoo 192.168.1.50  # Pixoo prüfen: 5 s „ON AIR“, dann zurück zum vorherigen Kanal
cargo run -p micled-cli -- hue pair 192.168.1.20  # nach dem Druck auf den Bridge-Knopf: Schlüssel und Fingerabdruck
MICLED_HUE_KEY=<schlüssel> MICLED_HUE_FINGERPRINT=<fingerabdruck> \
  cargo run -p micled-cli -- hue test 192.168.1.20 <lampen-id> [--room]   # 5 s Rot, dann zurück
```

## Aufbau

```
crates/micled-core   Lampe (USB/Webhook), Mikrofon und Kamera, Logik, Statusdatei, Nextcloud-Kalender, Bilder für den Pixoo 64, Hue Bridge
crates/micled-cli    Kommandozeile `micled`
crates/micled-ffi    UniFFI-Schicht für Swift
apps/MicLED          SwiftUI-Menüleisten-App, project.yml für xcodegen
scripts/             build-rust.sh, bump-build.sh, release.sh, screenshots.sh
docs/images/         Screenshots und Videos von Lampe und Pixoo (lamp.gif, pixoo.gif) für diese README
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
