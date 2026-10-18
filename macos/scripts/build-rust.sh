#!/usr/bin/env bash
# Builds the Rust player for the macOS app into macos/Generated:
#
#   BopsiePlayerFFI.xcframework   the static library and the C header its bindings call
#   Sources/bopsie_player_ffi.swift   the Swift bindings
#
# Xcode runs it before compiling, with CONFIGURATION and ARCHS set; run it by hand before the
# first build, since the project refers to what it makes. It needs cargo on PATH, and a Rust
# standard library for each architecture it builds (aarch64-apple-darwin, x86_64-apple-darwin).
set -euo pipefail

MACOS="$(cd "$(dirname "$0")/.." && pwd)"
PLAYER="$MACOS/../player"
OUT="$MACOS/Generated"
CRATE=bopsie_player_ffi

if ! command -v cargo >/dev/null 2>&1; then
  if [ -d "$OUT/BopsiePlayerFFI.xcframework" ]; then
    echo "warning: cargo is not on PATH; using the Rust player built before" >&2
    exit 0
  fi
  echo "error: cargo is not on PATH; run macos/scripts/build-rust.sh from a shell with Rust" >&2
  exit 1
fi

case "${CONFIGURATION:-Debug}" in
  Release) profile=release; cargo_profile=(--release) ;;
  *) profile=debug; cargo_profile=() ;;
esac

targets=()
for arch in ${ARCHS:-$(uname -m)}; do
  case "$arch" in
    arm64) targets+=(aarch64-apple-darwin) ;;
    x86_64) targets+=(x86_64-apple-darwin) ;;
    *) echo "error: no Rust target for $arch" >&2; exit 1 ;;
  esac
done

# Xcode's environment points the C toolchain at the app's SDK and deployment target, which is what
# the Rust build should use as well; outside Xcode the defaults apply.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"

libraries=()
for target in "${targets[@]}"; do
  cargo build --manifest-path "$PLAYER/Cargo.toml" -p bopsie-player-ffi --lib \
    --target "$target" "${cargo_profile[@]+"${cargo_profile[@]}"}"
  libraries+=("$PLAYER/target/$target/$profile/lib$CRATE.a")
done

work="$OUT/.work"
rm -rf "$work"
mkdir -p "$work/include" "$OUT/Sources"
lipo -create "${libraries[@]}" -output "$work/lib$CRATE.a"

cargo run --quiet --manifest-path "$PLAYER/Cargo.toml" -p bopsie-player-ffi --bin uniffi-bindgen -- \
  generate --library "$PLAYER/target/${targets[0]}/$profile/lib$CRATE.dylib" \
  --language swift --out-dir "$work/bindings"
cp "$work/bindings/${CRATE}FFI.h" "$work/include/"
cp "$work/bindings/${CRATE}FFI.modulemap" "$work/include/module.modulemap"
cp "$work/bindings/$CRATE.swift" "$OUT/Sources/$CRATE.swift"

rm -rf "$OUT/BopsiePlayerFFI.xcframework"
xcodebuild -create-xcframework -library "$work/lib$CRATE.a" -headers "$work/include" \
  -output "$OUT/BopsiePlayerFFI.xcframework" >/dev/null
rm -rf "$work"
