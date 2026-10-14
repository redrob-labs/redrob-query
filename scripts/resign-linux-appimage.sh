#!/usr/bin/env bash
# Repack the Linux AppImage without its over-bundled Wayland libraries, RE-SIGN it, re-upload it,
# and point latest.json at the new signature.
#
# Runs on the Linux release leg AFTER tauri-action has built, signed and uploaded. Each step
# exists because skipping it fails silently rather than loudly:
#
#   repack      without it the AppImage opens a blank window on Mesa 25+ (tauri#15665); see
#               strip-appimage-wayland.sh for the measurement
#   re-sign     the Tauri updater's Linux artifact IS the AppImage, and its .sig covers the
#               pre-repack bytes. A stale signature still LOOKS signed: the app installs, and the
#               first update check rejects every download forever
#   re-upload   the draft would otherwise carry the old AppImage and the old .sig
#   manifest    latest.json embeds the signature text itself, not a link to the .sig file, so
#               uploading a new .sig alone changes nothing a client reads
#
# Usage: resign-linux-appimage.sh <bundle-dir> <tag> <appimagetool>
# Env:   GH_TOKEN, GITHUB_REPOSITORY, TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD
set -euo pipefail

bundle_dir="${1:?usage: $0 <bundle-dir> <tag> <appimagetool>}"
tag="${2:?usage: $0 <bundle-dir> <tag> <appimagetool>}"
appimagetool="${3:?usage: $0 <bundle-dir> <tag> <appimagetool>}"
here="$(cd "$(dirname "$0")" && pwd)"
: "${GITHUB_REPOSITORY:?}" "${TAURI_SIGNING_PRIVATE_KEY:?}"

mapfile -t images < <(find "$bundle_dir/appimage" -maxdepth 1 -name '*.AppImage' | sort)
if [ "${#images[@]}" -ne 1 ]; then
  echo "::error::expected exactly one AppImage in $bundle_dir/appimage, found ${#images[@]}"
  exit 1
fi
image="${images[0]}"
name="$(basename "$image")"
# GitHub stores an uploaded asset under its name with every space turned into a dot
# ("Redrob Query_0.2.2_amd64.AppImage" becomes "Redrob.Query_0.2.2_amd64.AppImage"), so the
# release is searched for that form; the local file keeps its own name.
asset_name="${name// /.}"

bash "$here/strip-appimage-wayland.sh" "$image" "$appimagetool"

rm -f "$image.sig"
npx --no-install tauri signer sign "$image"
[ -s "$image.sig" ] || { echo "::error::tauri signer produced no $name.sig"; exit 1; }

# The release is a DRAFT, which the tags endpoint does not resolve, so find it in the listing.
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
release_assets() {
  gh api "repos/${GITHUB_REPOSITORY}/releases?per_page=100" \
    | jq --arg tag "$tag" 'map(select(.tag_name == $tag)) | first | .assets'
}
asset_id_named() {
  jq -r --arg name "$2" 'map(select(.name == $name)) | first | .id // empty' "$1"
}

# On a draft, tauri-action writes REST asset URLs into latest.json
# (https://api.github.com/repos/<repo>/releases/assets/<id>) -- a draft asset has no public
# download URL yet -- so the manifest knows this AppImage by its asset ID, not its name. Record
# that ID before the upload below replaces the asset and mints a new one.
release_assets > "$work/assets.before.json"
old_id="$(asset_id_named "$work/assets.before.json" "$asset_name")"
if [ -z "$old_id" ]; then
  echo "::error::$name is not attached to $tag; tauri-action did not upload it"
  exit 1
fi

gh release upload "$tag" "$image" "$image.sig" --clobber --repo "$GITHUB_REPOSITORY"

release_assets > "$work/assets.after.json"
new_id="$(asset_id_named "$work/assets.after.json" "$asset_name")"
manifest_id="$(asset_id_named "$work/assets.after.json" "latest.json")"
if [ -z "$new_id" ]; then
  echo "::error::$name is not attached to $tag after re-upload"
  exit 1
fi
if [ -z "$manifest_id" ]; then
  echo "::error::latest.json is not attached to $tag yet; the manifest cannot be updated"
  exit 1
fi
gh api -H 'Accept: application/octet-stream' \
  "repos/${GITHUB_REPOSITORY}/releases/assets/${manifest_id}" > "$work/latest.json"

# Repoint every platform entry that named this AppImage -- by the old asset ID, or by name should
# a published-form URL ever appear -- at the re-uploaded asset, with the new signature.
# tauri-action writes both `linux-x86_64` and `linux-x86_64-appimage`, and missing one leaves a
# client that resolves the other key verifying against stale bytes.
sig="$(cat "$image.sig")"
new_url="https://api.github.com/repos/${GITHUB_REPOSITORY}/releases/assets/${new_id}"
jq --arg name "$asset_name" --arg sig "$sig" --arg old_id "$old_id" --arg new_url "$new_url" '
  .platforms |= with_entries(
    if (.value.url | (endswith("/releases/assets/" + $old_id) or endswith("/" + $name)))
    then .value.url = $new_url | .value.signature = $sig
    else . end)
' "$work/latest.json" > "$work/latest.new.json"

replaced="$(jq --arg url "$new_url" --arg sig "$sig" \
  '[.platforms[] | select(.url == $url and .signature == $sig)] | length' \
  "$work/latest.new.json")"
if [ "$replaced" -lt 1 ]; then
  echo "::error::no latest.json platform entry points at $name (asset $old_id); nothing was re-signed in the manifest"
  exit 1
fi

cp "$work/latest.new.json" "$work/latest.json"
gh release upload "$tag" "$work/latest.json" --clobber --repo "$GITHUB_REPOSITORY"
echo "re-signed $name and updated $replaced latest.json platform entr$( [ "$replaced" -eq 1 ] && echo y || echo ies)"
