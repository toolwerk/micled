#!/usr/bin/env bash
# Richtet MicLED auf einem Mac ein: Werkzeuge prüfen, Rust-Kern bauen,
# Xcode-Projekt erzeugen. Ändert weder Git-Einstellungen noch das Repo auf GitHub.
set -euo pipefail
cd "$(dirname "$0")"

need() { command -v "$1" >/dev/null || { echo "Fehlt: $1 ($2)"; exit 1; }; }
need xcodebuild "Xcode installieren, einmal öffnen und Lizenz bestätigen"
need xcodegen "brew install xcodegen"
need rustup "https://rustup.rs"

rustup target add aarch64-apple-darwin x86_64-apple-darwin

cargo test --workspace
./scripts/build-rust.sh
(cd apps/MicLED && xcodegen generate)

cat <<MSG

Fertig.
  App:      open apps/MicLED/MicLED.xcodeproj  (Schema MicLED, ⌘R)
  CLI:      cargo run -p micled-cli -- mic
            cargo run -p micled-cli -- watch
  Release:  scripts/release.sh
MSG
