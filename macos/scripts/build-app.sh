#!/usr/bin/env bash
# Builds Bopsie.app: the Rust player, the Xcode project, then the app, into macos/build.
#
#   scripts/build-app.sh            # Debug
#   scripts/build-app.sh Release
#
# Needs Xcode, XcodeGen and cargo on PATH; the development shell provides the last two.
set -euo pipefail

MACOS="$(cd "$(dirname "$0")/.." && pwd)"
CONFIGURATION="${1:-Debug}"

cd "$MACOS"
CONFIGURATION="$CONFIGURATION" scripts/build-rust.sh
xcodegen --quiet
xcodebuild -project Bopsie.xcodeproj -scheme Bopsie -configuration "$CONFIGURATION" \
  -destination "platform=macOS,arch=$(uname -m)" -derivedDataPath build -quiet build
echo "$MACOS/build/Build/Products/$CONFIGURATION/Bopsie.app"
