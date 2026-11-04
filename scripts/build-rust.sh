#!/usr/bin/env bash
# Baut den Rust-Kern als universelle statische Bibliothek, erzeugt die
# Swift-Bindings (UniFFI) und packt alles in build/MicLEDCore.xcframework.
# Läuft als Pre-Build-Phase in Xcode und einmal aus setup.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Xcode setzt ein schmales PATH und MACOSX_DEPLOYMENT_TARGET.
# Deployment-Target fest auf das der App (project.yml) setzen, sonst baut der
# cc-Crate C-Teile (ring, hidapi) für die Host-Version und ld warnt beim Linken.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=14.0

PROFILE="${MICLED_RUST_PROFILE:-release}"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
GEN="apps/MicLED/Generated"
OUT="build"

for t in "${TARGETS[@]}"; do
  rustup target add "$t" >/dev/null 2>&1 || true
  cargo build -p micled-ffi --profile "$PROFILE" --target "$t"
done

DIR=$([ "$PROFILE" = "dev" ] && echo debug || echo "$PROFILE")
mkdir -p "$OUT/universal" "$OUT/include" "$OUT/bindings" "$GEN"
lipo -create \
  "target/aarch64-apple-darwin/$DIR/libmicled_ffi.a" \
  "target/x86_64-apple-darwin/$DIR/libmicled_ffi.a" \
  -output "$OUT/universal/libmicled_ffi.a"

cargo run -q -p micled-ffi --bin uniffi-bindgen -- generate \
  --library "target/aarch64-apple-darwin/$DIR/libmicled_ffi.dylib" \
  --language swift --out-dir "$OUT/bindings"

# Nur ersetzen, wenn sich etwas geändert hat (sonst baut Xcode jedes Mal alles neu)
install_if_changed() { cmp -s "$1" "$2" || cp "$1" "$2"; }
install_if_changed "$OUT/bindings/micled_ffi.swift" "$GEN/micled_ffi.swift"
install_if_changed "$OUT/bindings/micled_ffiFFI.h" "$OUT/include/micled_ffiFFI.h"
install_if_changed "$OUT/bindings/micled_ffiFFI.modulemap" "$OUT/include/module.modulemap"

rm -rf "$OUT/MicLEDCore.xcframework"
xcodebuild -create-xcframework \
  -library "$OUT/universal/libmicled_ffi.a" -headers "$OUT/include" \
  -output "$OUT/MicLEDCore.xcframework" >/dev/null

echo "Rust-Kern fertig: $OUT/MicLEDCore.xcframework"
