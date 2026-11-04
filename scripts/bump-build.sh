#!/usr/bin/env bash
# Zählt CURRENT_PROJECT_VERSION in project.yml hoch. Build-Nummern werden nie zurückgesetzt.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
YML="$ROOT/apps/MicLED/project.yml"
cur=$(sed -n 's/^ *CURRENT_PROJECT_VERSION: "\([0-9]*\)".*/\1/p' "$YML")
next=$((cur + 1))
sed -i '' "s/CURRENT_PROJECT_VERSION: \"$cur\"/CURRENT_PROJECT_VERSION: \"$next\"/" "$YML"
(cd "$ROOT/apps/MicLED" && xcodegen generate --quiet)
echo "Build $cur -> $next"
