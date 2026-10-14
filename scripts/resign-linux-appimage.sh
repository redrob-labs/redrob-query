#!/usr/bin/env bash
# Repack the Linux AppImage without its over-bundled Wayland libraries and RE-SIGN it, in place.
#
# Runs on the Linux release leg after `tauri build` and BEFORE the bundle is staged into the
# release artifact. Each step exists because skipping it fails silently rather than loudly:
#
#   repack      without it the AppImage opens a blank window on Mesa 25+ (tauri#15665); see
#               strip-appimage-wayland.sh for the measurement
#   re-sign     the Tauri updater's Linux artifact IS the AppImage, and its .sig covers the
#               pre-repack bytes. A stale signature still LOOKS signed: the app installs, and the
#               first update check rejects every download forever
#
# latest.json is written later, by write-updater-manifest.mjs, from whatever .sig sits beside the
# staged file -- so the new signature reaches the manifest without this script knowing about it,
# and that job verifies every signature against the bytes it names before anything is published.
#
# Usage: resign-linux-appimage.sh <bundle-dir> <appimagetool>
# Env:   TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD
set -euo pipefail

bundle_dir="${1:?usage: $0 <bundle-dir> <appimagetool>}"
appimagetool="${2:?usage: $0 <bundle-dir> <appimagetool>}"
here="$(cd "$(dirname "$0")" && pwd)"
: "${TAURI_SIGNING_PRIVATE_KEY:?}"

mapfile -t images < <(find "$bundle_dir/appimage" -maxdepth 1 -name '*.AppImage' | sort)
if [ "${#images[@]}" -ne 1 ]; then
  echo "::error::expected exactly one AppImage in $bundle_dir/appimage, found ${#images[@]}"
  exit 1
fi
image="${images[0]}"
name="$(basename "$image")"

bash "$here/strip-appimage-wayland.sh" "$image" "$appimagetool"

rm -f "$image.sig"
npx --no-install tauri signer sign "$image"
[ -s "$image.sig" ] || { echo "::error::tauri signer produced no $name.sig"; exit 1; }

echo "repacked and re-signed $name"
