#!/usr/bin/env bash
# Screenshots für die README neu erzeugen (docs/images/*-light.png, *-dark.png).
# Startet die Debug-App im Screenshot-Modus mit Beispieldaten; Lampe, Slack
# und Nextcloud bleiben unberührt. Das Terminal braucht einmalig das Recht
# „Bildschirmaufnahme“ (Systemeinstellungen › Datenschutz & Sicherheit).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/docs/images"
DERIVED="$ROOT/build/screens"
APP="$DERIVED/Build/Products/Debug/MicLED.app"
mkdir -p "$OUT"

"$ROOT/scripts/build-rust.sh" >/dev/null
(cd "$ROOT/apps/MicLED" && xcodegen generate --quiet)
xcodebuild -project "$ROOT/apps/MicLED/MicLED.xcodeproj" -scheme MicLED -configuration Debug \
  -derivedDataPath "$DERIVED" build >/dev/null 2>&1 || { echo "Debug-Build fehlgeschlagen" >&2; exit 1; }

# Fenstertitel → Dateiname (settings-network: Einstellungen bei Govee und Nanoleaf,
# settings-wled: bei WLED, settings-strip: beim Lichtstreifen)
WINDOWS=("MicLED-Menü:menu" "Statistik:stats" "MicLED:settings" "MicLED:settings-network" "MicLED:settings-wled" "MicLED:settings-strip")

# je Fenster ein eigener Start, damit es das aktive Fenster ist
for look in light dark; do
  for entry in "${WINDOWS[@]}"; do
    title="${entry%%:*}" name="${entry##*:}"
    args=(--window "$name")
    [[ "$name" == settings-network ]] && args=(--window settings --scroll network)
    [[ "$name" == settings-wled ]] && args=(--window settings --scroll wled)
    [[ "$name" == settings-strip ]] && args=(--window settings --scroll strip)
    "$APP/Contents/MacOS/MicLED" --screenshots --appearance "$look" "${args[@]}" >/dev/null 2>&1 &
    pid=$!
    trap 'kill $pid 2>/dev/null || true' EXIT
    sleep 4
    id=$(swift "$ROOT/scripts/window-id.swift" "$pid" "$title") || {
      echo "Fenster „$title“ nicht gefunden" >&2; exit 1; }
    screencapture -l"$id" -o "$OUT/$name-$look.png"
    sips -Z 1400 "$OUT/$name-$look.png" >/dev/null
    echo "$OUT/$name-$look.png"
    kill "$pid"
    wait "$pid" 2>/dev/null || true
    trap - EXIT
  done
done
