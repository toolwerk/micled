#!/bin/sh
# Übersetzungen nachführen und übernehmen (crates/micled-i18n, TRANSLATING.md).
#
#   scripts/translations.sh                     Vorlage und alle Sprachen auf den Stand der Texte bringen
#   scripts/translations.sh import fr.po        ausgefüllte Datei als translations/fr.po übernehmen
#   scripts/translations.sh import x.po --lang fr
#
# Nach jeder Textänderung in App oder Kern laufen lassen; die Tests schlagen
# sonst fehl. Der Dateipfad beim Import gilt vom aktuellen Ordner aus.
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec cargo run -q --manifest-path "$ROOT/Cargo.toml" -p micled-i18n -- --root "$ROOT" "$@"
