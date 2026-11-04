#!/usr/bin/env bash
# Baut, signiert (Developer ID), notarisiert und prüft MicLED.
# Ergebnis: dist/MicLED-<Version>.zip
#
# Einmalig vorher:
#   xcrun notarytool store-credentials micled --apple-id <Apple-ID> --team-id <TEAM> --password <app-spezifisch>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT/apps/MicLED"
PROFILE="${NOTARY_PROFILE:-micled}"
BUILD="$ROOT/build/release"

fail() { echo "Abbruch: $*" >&2; exit 1; }

# 1. Voraussetzungen prüfen, bevor irgendetwas gebaut wird
command -v xcodegen >/dev/null || fail "xcodegen fehlt (brew install xcodegen)"
command -v cargo >/dev/null || fail "Rust fehlt (rustup)"
security find-identity -v -p codesigning | grep -q "Developer ID Application" \
  || fail "Kein Developer-ID-Zertifikat im Schlüsselbund"
xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1 \
  || fail "notarytool-Profil '$PROFILE' fehlt (siehe Kopf dieses Skripts)"
grep -q 'DEVELOPMENT_TEAM: ""' "$APP_DIR/project.yml" && fail "DEVELOPMENT_TEAM in project.yml eintragen"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || fail "Git-Arbeitsstand ist nicht sauber"

VERSION=$(sed -n 's/^ *MARKETING_VERSION: "\(.*\)".*/\1/p' "$APP_DIR/project.yml")
BUILDNO=$(sed -n 's/^ *CURRENT_PROJECT_VERSION: "\(.*\)".*/\1/p' "$APP_DIR/project.yml")
echo "Release $VERSION ($BUILDNO)"

# 2. Bauen
rm -rf "$BUILD" && mkdir -p "$BUILD" "$ROOT/dist"
"$ROOT/scripts/build-rust.sh"
(cd "$APP_DIR" && xcodegen generate --quiet)
xcodebuild -project "$APP_DIR/MicLED.xcodeproj" -scheme MicLED -configuration Release \
  -archivePath "$BUILD/MicLED.xcarchive" archive -quiet || fail "Archiv fehlgeschlagen"

cat > "$BUILD/ExportOptions.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>method</key><string>developer-id</string>
  <key>signingStyle</key><string>manual</string>
</dict></plist>
PLIST
xcodebuild -exportArchive -archivePath "$BUILD/MicLED.xcarchive" \
  -exportOptionsPlist "$BUILD/ExportOptions.plist" -exportPath "$BUILD/export"
APP="$BUILD/export/MicLED.app"

# 3. Notarisieren und stapeln
ditto -c -k --keepParent "$APP" "$BUILD/MicLED-notarize.zip"
xcrun notarytool submit "$BUILD/MicLED-notarize.zip" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$APP"

# 4. Prüfen
codesign -dv --verbose=2 "$APP" 2>&1 | grep -E "Authority=Developer ID|flags=.*runtime" \
  || fail "Signatur oder Hardened Runtime fehlt"
spctl -a -t exec -vv "$APP"
xcrun stapler validate "$APP"

OUT="$ROOT/dist/MicLED-$VERSION.zip"
ditto -c -k --keepParent "$APP" "$OUT"
echo "Fertig: $OUT"
