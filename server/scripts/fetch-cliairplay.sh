#!/usr/bin/env bash
# Downloads the pinned cliairplay release for this platform and checks its SHA-256.
#
#   scripts/fetch-cliairplay.sh [destination]   # default: ./cliairplay
#
# cliairplay is GPL-3.0 and runs as a separate process, so it is fetched rather than built in.
# To move to a new release, update VERSION and the checksums from its SHA256SUMS asset.
set -euo pipefail

VERSION="v0.5.7"
DEST="${1:-cliairplay}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ASSET=cliairplay-linux-x86_64
    SHA256=6d1a1ab1502c4cfde5d98f4426e73575b1dda8767ee553e7d07b6dd1df64c23a ;;
  Linux-aarch64 | Linux-arm64) ASSET=cliairplay-linux-aarch64
    SHA256=d09ba736a2cc0d236212a7cc7787a253ce82f66f2a75f68701de45b53cba8ae6 ;;
  Darwin-arm64) ASSET=cliairplay-macos-arm64
    SHA256=05cf8ae76ee4d6ebeeff41f7ef19dfb3d7aead5e1c05b009dc7f0c1fac08550f ;;
  Darwin-x86_64) ASSET=cliairplay-macos-x86_64
    SHA256=d70668077e660474b5458433b7815ba55cc31d701b5eaa4e6959f71b581be01c ;;
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
