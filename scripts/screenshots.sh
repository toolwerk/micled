#!/usr/bin/env bash
# Screenshots für die README neu erzeugen (docs/images/*-light.png, *-dark.png),
# auf Englisch für README.en.md nach docs/images/en/.
# Startet die Debug-App im Screenshot-Modus mit Beispieldaten; Lampe, Slack
# und Nextcloud bleiben unberührt. Das Terminal braucht einmalig das Recht
# „Bildschirmaufnahme“ (Systemeinstellungen › Datenschutz & Sicherheit).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/docs/images"
DERIVED="$ROOT/build/screens"
APP="$DERIVED/Build/Products/Debug/MicLED.app"
mkdir -p "$OUT" "$OUT/en"

"$ROOT/scripts/build-rust.sh" >/dev/null
(cd "$ROOT/apps/MicLED" && xcodegen generate --quiet)
xcodebuild -project "$ROOT/apps/MicLED/MicLED.xcodeproj" -scheme MicLED -configuration Debug \
  -derivedDataPath "$DERIVED" build >/dev/null 2>&1 || { echo "Debug-Build fehlgeschlagen" >&2; exit 1; }

# Fenstertitel → Dateiname (stats-week: Statistik in der Ansicht „Wochen“,
# settings-nextcloud: Einstellungen bei Nextcloud,
# settings-network: bei Govee und Nanoleaf, settings-wled: bei WLED, settings-strip: beim Lichtstreifen,
# settings-mqtt: bei Home Assistant per MQTT). Deutsch nach docs/images/, Englisch nach docs/images/en/; die Einstellungen heißen in beiden Sprachen „MicLED“.
WINDOWS_DE=("MicLED-Menü:menu" "Statistik:stats" "Statistik:stats-week")
WINDOWS_EN=("MicLED Menu:menu" "Statistics:stats" "Statistics:stats-week")
SETTINGS=("MicLED:settings" "MicLED:settings-nextcloud" "MicLED:settings-network" "MicLED:settings-wled" "MicLED:settings-strip" "MicLED:settings-mqtt")

# je Fenster ein eigener Start, damit es das aktive Fenster ist
for lang in de en; do
  if [[ "$lang" == de ]]; then dir="$OUT" windows=("${WINDOWS_DE[@]}" "${SETTINGS[@]}")
  else dir="$OUT/en" windows=("${WINDOWS_EN[@]}" "${SETTINGS[@]}"); fi
  for look in light dark; do
    for entry in "${windows[@]}"; do
      title="${entry%%:*}" name="${entry##*:}"
      args=(--window "$name")
      [[ "$name" == stats-week ]] && args=(--window stats --stats week)
      [[ "$name" == settings-nextcloud ]] && args=(--window settings --scroll nextcloud)
      [[ "$name" == settings-network ]] && args=(--window settings --scroll network)
      [[ "$name" == settings-wled ]] && args=(--window settings --scroll wled)
      [[ "$name" == settings-strip ]] && args=(--window settings --scroll strip)
      [[ "$name" == settings-mqtt ]] && args=(--window settings --scroll mqtt)
      "$APP/Contents/MacOS/MicLED" --screenshots --appearance "$look" "${args[@]}" \
        -AppleLanguages "($lang)" >/dev/null 2>&1 &
      pid=$!
      trap 'kill $pid 2>/dev/null || true' EXIT
      sleep 4
      id=$(swift "$ROOT/scripts/window-id.swift" "$pid" "$title") || {
        echo "Fenster „$title“ nicht gefunden" >&2; exit 1; }
      screencapture -l"$id" -o "$dir/$name-$look.png"
      sips -Z 1400 "$dir/$name-$look.png" >/dev/null
      echo "$dir/$name-$look.png"
      kill "$pid"
      wait "$pid" 2>/dev/null || true
      trap - EXIT
    done
  done
done
