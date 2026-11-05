#!/usr/bin/env bash
# Richtet MicLED auf einem Mac ein: Werkzeuge prüfen, Rust-Kern bauen,
# Xcode-Projekt erzeugen. Mit --github zusätzlich privates Repo anlegen und pushen.
set -euo pipefail
cd "$(dirname "$0")"

need() { command -v "$1" >/dev/null || { echo "Fehlt: $1 ($2)"; exit 1; }; }
need xcodebuild "Xcode installieren, einmal öffnen und Lizenz bestätigen"
need xcodegen "brew install xcodegen"
need rustup "https://rustup.rs"

rustup target add aarch64-apple-darwin x86_64-apple-darwin

if [ ! -d .git ]; then
  git init -b main
fi
git config user.name "Frank Neumann-Staude"
git config user.email "frank@staude.net"

cargo test --workspace
./scripts/build-rust.sh
(cd apps/MicLED && xcodegen generate)

if [ "${1:-}" = "--github" ]; then
  need gh "brew install gh"
  git add -A
  git diff --cached --quiet || git commit -m "Erster Stand: Rust-Kern, CLI, Menüleisten-App"
  gh repo create toolwerk/micled --private --source . --push
fi

cat <<MSG

Fertig.
  App:      open apps/MicLED/MicLED.xcodeproj  (Schema MicLED, ⌘R)
  CLI:      cargo run -p micled-cli -- mic
            cargo run -p micled-cli -- watch
  Release:  scripts/release.sh
MSG
