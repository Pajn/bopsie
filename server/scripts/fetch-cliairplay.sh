#!/usr/bin/env bash
# Downloads the pinned cliairplay release for this platform and checks its SHA-256.
#
#   scripts/fetch-cliairplay.sh [destination]   # default: ./cliairplay
#
# cliairplay is GPL-3.0 and runs as a separate process, so it is fetched rather than built in.
# To move to a new release, update VERSION and the checksums from its SHA256SUMS asset.
set -euo pipefail

VERSION="v0.5.8"
DEST="${1:-cliairplay}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ASSET=cliairplay-linux-x86_64
    SHA256=0b33f68cdbb32a658cd50124cdb558e141247e0ff845a19474194a6e839d5500 ;;
  Linux-aarch64 | Linux-arm64) ASSET=cliairplay-linux-aarch64
    SHA256=73cfae9328d8882114cc788bd15676fc1dec844ff87ca832fb602818cc1b409d ;;
  Darwin-arm64) ASSET=cliairplay-macos-arm64
    SHA256=6d460ad08562844d978f337bee28a79b0eea390c84c239b5d66f55624e27d719 ;;
  Darwin-x86_64) ASSET=cliairplay-macos-x86_64
    SHA256=589dfb70112dd1a5bbebf86ecd1a9ac3cb0d4d9114f84ccb942ecb324e8a6455 ;;
  *) echo "No cliairplay build for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
curl -fsSL "https://github.com/music-assistant/airplay-cli/releases/download/${VERSION}/${ASSET}" -o "$TMP"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$TMP" | cut -d' ' -f1)"
else
  ACTUAL="$(shasum -a 256 "$TMP" | cut -d' ' -f1)"
fi
if [ "$ACTUAL" != "$SHA256" ]; then
  echo "Checksum mismatch for ${ASSET}: expected ${SHA256}, got ${ACTUAL}" >&2
  exit 1
fi

install -m 755 "$TMP" "$DEST"
"$DEST" --check
