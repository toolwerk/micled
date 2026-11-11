# MicLED - Hinweise für Claude

Menüleisten-App für macOS: schaltet eine Luxafor-Lampe bei aktivem Mikrofon um. Projekt-Standards aus dem Vault (`00 Kontext/Projekt-Standards.md`) gelten, Referenzprojekt für den Aufbau ist Amiscara Agents.

## Architektur

- Logik gehört in `crates/micled-core`, nicht in Swift. Die App zeigt nur an und reicht Einstellungen durch.
- `engine.rs` ist seitenwirkungsfrei und vollständig getestet. Jede Verhaltensänderung bekommt dort zuerst einen Test.
- `monitor.rs` ist der einzige Ort mit Thread, Zeit und Ein-/Ausgabe, außerdem die Updater in `presence.rs` (Status in Slack und Nextcloud, Netz in `slack.rs` und `nc_status.rs`) und `output.rs`, die Netzwerkzugriffe im eigenen Thread abarbeiten. Die Geräte dafür (`pixoo.rs`, `awtrix.rs`, `hue.rs` mit `hue_net.rs`, `govee.rs`, `nanoleaf.rs`, `wled.rs`, `robobloq.rs`) halten ihre Logik testbar, das Senden wird in den Tests ersetzt. Mehrere Geräte einer Art laufen als `Fleet` aus `output.rs` in einem Updater; ein Gerät darin meldet ein gescheitertes Zurückstellen mit `Err` und behält seinen Zustand, damit `Fleet` es wiederholt.
- Govee antwortet immer an Port 4002 des Macs. `govee.rs` belegt ihn nur für einen Vorgang und nie zweimal gleichzeitig (`REPLY_LOCK`). Die Nanoleaf- und WLED-Suche (`bonjour` in `output.rs`, `mdns-sd`) startet einen eigenen Thread, nie aus einem Ausgabe-Thread aufrufen.
- USB: Gerät nach jedem Befehl schließen und `macos-shared-device` behalten. Sonst blockiert MicLED die Luxafor-App, das Stream-Deck-Plugin und `lux` (`DeviceNotFound`). Das gilt auch für den Lichtstreifen in `robobloq.rs`; dort nur die Schnittstelle mit Usage Page `0xFF00` öffnen, die Tastatur-Schnittstelle verlangt „Eingabeüberwachung“. Ob der Streifen angesteckt ist, prüft `monitor.rs` alle fünf Sekunden.
- Vor jeder Farbe wird `00 4F` gesendet und die Farbe doppelt geschrieben. Ohne das ignoriert die Lampe Farbbefehle, solange ein Muster läuft.
- Statusdatei `~/Library/Application Support/net.staude.luxafor/state.json` ist ein Vertrag mit dem Stream-Deck-Plugin (`staude/luxafor-streamdeck`). Felder `color` (rrggbb), `source`, `updated`. Änderungen mit beiden Seiten abstimmen.
- Die Mikrofarbe wird nie in die Statusdatei geschrieben, nur ruhende Farben.

## Swift

- Xcode-Projekt nur über `apps/MicLED/project.yml` und `xcodegen generate` ändern.
- Rust baut im Aggregate-Target `RustCore` (nicht `MicLEDCore` nennen). Bibliothek und Header direkt einbinden, kein XCFramework, und explizite Module aus lassen. Sonst baut Xcode nach einer API-Änderung gegen den alten Header.
- `apps/MicLED/Generated/` und `build/` sind generiert und nicht eingecheckt.
- Updates laufen über Sparkle in `UpdaterController.swift`, nicht im Rust-Kern, weil Sparkle ein AppKit-Framework ist. Den EdDSA-Schlüssel (Schlüsselbund, Konto `micled`) nie ersetzen, sonst nehmen installierte Apps keine Updates mehr an. Sparkle vergleicht `CFBundleVersion`, jedes Release braucht eine höhere Build-Nummer.
- Texte: Deutsch ist die Quellsprache, Englisch die einzige Übersetzung; was nicht Deutsch ist, zeigt Englisch. Neue Texte der App kommen mit englischer Übersetzung (und ausdrücklichem `de`-Eintrag) in `Resources/Localizable.xcstrings`. Ein `String`, der angezeigt wird, kommt aus `String(localized:)`, weil SwiftUI `Text(variable)` nicht übersetzt. Fehlertexte im Kern laufen über `tr!("deutsch", "english")` aus `lang.rs`, die App stellt die Sprache beim Start mit `setLanguage` ein.
- Swift 6, macOS 14. Statusmeldungen aus Rust kommen auf einem Hintergrund-Thread und werden per `Task { @MainActor in }` übergeben.

## Prüfen

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p micled-cli -- mic
```

Nach sichtbaren Änderungen an Menü, Einstellungen oder Statistik `scripts/screenshots.sh` laufen lassen und die Bilder in `docs/images/` mit einchecken.
