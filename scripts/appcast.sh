#!/usr/bin/env bash
# Signiert das ZIP mit dem Sparkle-Schlüssel und schreibt appcast.xml daneben.
# scripts/release.sh ruft das am Ende auf.
#
#   scripts/appcast.sh <zip> <version> <build>
#
# Der Schlüssel liegt im Anmelde-Schlüsselbund (Konto micled). SIGN_UPDATE
# ersetzt das Werkzeug, damit sich das Skript ohne Schlüssel prüfen lässt.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO_URL="https://github.com/toolwerk/micled"

fail() { echo "Abbruch: $*" >&2; exit 1; }

[ $# -eq 3 ] || fail "Aufruf: scripts/appcast.sh <zip> <version> <build>"
ZIP="$1" VERSION="$2" BUILDNO="$3"
[ -f "$ZIP" ] || fail "ZIP fehlt: $ZIP"

SIGN_UPDATE="${SIGN_UPDATE:-$ROOT/build/spm/artifacts/sparkle/Sparkle/bin/sign_update}"
[ -x "$SIGN_UPDATE" ] || fail "sign_update fehlt: $SIGN_UPDATE (kommt mit dem Sparkle-Paket, README, Abschnitt „Release“)"

# Ausgabe: sparkle:edSignature="…" length="…"
SIGNED=$("$SIGN_UPDATE" --account micled "$ZIP") || fail "sign_update ist fehlgeschlagen"
SIGNATURE=$(printf '%s\n' "$SIGNED" | sed -nE 's/.*sparkle:edSignature="([^"]+)".*/\1/p')
LENGTH=$(printf '%s\n' "$SIGNED" | sed -nE 's/.*length="([0-9]+)".*/\1/p')
[ -n "$SIGNATURE" ] && [ -n "$LENGTH" ] || fail "Ausgabe von sign_update nicht lesbar: $SIGNED"

MIN_SYSTEM=$(sed -n 's/^ *macOS: "\(.*\)".*/\1/p' "$ROOT/apps/MicLED/project.yml" | head -1)
[ -n "$MIN_SYSTEM" ] || fail "deploymentTarget macOS nicht in project.yml gefunden"

OUT="$(dirname "$ZIP")/appcast.xml"
cat > "$OUT" <<XML
<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>MicLED</title>
    <link>$REPO_URL</link>
    <description>Signierte Versionen von MicLED</description>
    <language>de</language>
    <item>
      <title>Version $VERSION</title>
      <pubDate>$(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S +0000')</pubDate>
      <sparkle:version>$BUILDNO</sparkle:version>
      <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>$MIN_SYSTEM</sparkle:minimumSystemVersion>
      <sparkle:releaseNotesLink>$REPO_URL/releases/tag/v$VERSION</sparkle:releaseNotesLink>
      <enclosure url="$REPO_URL/releases/download/v$VERSION/$(basename "$ZIP")" length="$LENGTH" type="application/octet-stream" sparkle:edSignature="$SIGNATURE"/>
    </item>
  </channel>
</rss>
XML
echo "$OUT"
