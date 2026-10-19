#!/usr/bin/env bash
# Builds Bopsie.app for distribution: signed with the team's Developer ID, notarized by Apple and
# stapled, then zipped into macos/build/Bopsie.zip.
#
# Needs, in the login Keychain:
#   - the team's Developer ID Application certificate and its private key;
#   - notarization credentials saved by `xcrun notarytool store-credentials`, under the profile
#     name in BOPSIE_NOTARY_PROFILE (default bopsie-notary).
# Neither lives in the repository.
set -euo pipefail

MACOS="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${BOPSIE_NOTARY_PROFILE:-bopsie-notary}"

app="$("$MACOS/scripts/build-app.sh" Release | tail -1)"
codesign --verify --strict --verbose=1 "$app"

zip="$MACOS/build/Bopsie.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
# notarytool exits successfully even when Apple rejects the app, so read the verdict.
result="$(xcrun notarytool submit "$zip" --keychain-profile "$PROFILE" --wait \
  --output-format json)"
id="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' <<<"$result")"
status="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' <<<"$result")"
if [ "$status" != "Accepted" ]; then
  echo "error: notarization: $status" >&2
  xcrun notarytool log "$id" --keychain-profile "$PROFILE" >&2
  exit 1
fi
xcrun stapler staple "$app"

# Zipped again, so the archive carries the stapled ticket and opens offline.
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
spctl --assess --type execute --verbose=1 "$app"
echo "$zip"
