# MicLED - Hinweise für Claude

Menüleisten-App für macOS: schaltet eine Luxafor-Lampe bei aktivem Mikrofon um. Projekt-Standards aus dem Vault (`00 Kontext/Projekt-Standards.md`) gelten, Referenzprojekt für den Aufbau ist Amiscara Agents.

## Architektur

- Logik gehört in `crates/micled-core`, nicht in Swift. Die App zeigt nur an und reicht Einstellungen durch.
- `engine.rs` ist seitenwirkungsfrei und vollständig getestet. Jede Verhaltensänderung bekommt dort zuerst einen Test.
- `monitor.rs` ist der einzige Ort mit Thread, Zeit und Ein-/Ausgabe.
- USB: Gerät nach jedem Befehl schließen und `macos-shared-device` behalten. Sonst blockiert MicLED die Luxafor-App, das Stream-Deck-Plugin und `lux` (`DeviceNotFound`).
- Vor jeder Farbe wird `00 4F` gesendet und die Farbe doppelt geschrieben. Ohne das ignoriert die Lampe Farbbefehle, solange ein Muster läuft.
- Statusdatei `~/Library/Application Support/net.staude.luxafor/state.json` ist ein Vertrag mit dem Stream-Deck-Plugin (`staude/luxafor-streamdeck`). Felder `color` (rrggbb), `source`, `updated`. Änderungen mit beiden Seiten abstimmen.
- Die Mikrofarbe wird nie in die Statusdatei geschrieben, nur ruhende Farben.

## Swift

- Xcode-Projekt nur über `apps/MicLED/project.yml` und `xcodegen generate` ändern.
- `apps/MicLED/Generated/` und `build/` sind generiert und nicht eingecheckt.
- Swift 6, macOS 14. Statusmeldungen aus Rust kommen auf einem Hintergrund-Thread und werden per `Task { @MainActor in }` übergeben.

## Prüfen

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p micled-cli -- mic
```
