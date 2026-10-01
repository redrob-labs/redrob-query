#!/usr/bin/env bash
# Remove the Wayland client libraries the Tauri AppImage bundler over-bundles, then repack.
#
# Why: the default bundler copies the BUILD host's libwayland-client/-cursor/-egl/-server into the
# AppImage. On a system whose Mesa is newer (Mesa 25+), the host's libEGL_mesa is loaded against
# that stale libwayland, eglGetDisplay(EGL_DEFAULT_DISPLAY) returns EGL_BAD_PARAMETER, and
# WebKitWebProcess prints "Could not create default EGL display: EGL_BAD_PARAMETER. Aborting..."
# and dies. The window opens and stays blank. Upstream: tauri-apps/tauri#15665.
#
# Measured on our own release before writing this: the v0.2.0 AppImage aborted exactly that way,
# and the same AppImage with only these four files moved aside rendered with no environment
# overrides at all. The WebProcess's stderr is swallowed by the parent, so the shell's own log
# shows nothing -- strace on the child is what found it.
#
# Removing them is safe: these are protocol client libraries every system with a Wayland or X11
# desktop already provides at the version its own Mesa expects. Nothing else is touched.
#
# Usage: strip-appimage-wayland.sh <path/to/App.AppImage> <path/to/appimagetool>
# The AppImage is replaced in place. Any .sig next to it is now stale and must be regenerated
# by the caller -- this script deliberately does not sign, so it never needs the private key.
set -euo pipefail

appimage="${1:?usage: $0 <AppImage> <appimagetool>}"
appimagetool="${2:?usage: $0 <AppImage> <appimagetool>}"
[ -f "$appimage" ] || { echo "no such AppImage: $appimage" >&2; exit 1; }
[ -x "$appimagetool" ] || { echo "appimagetool not executable: $appimagetool" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
appimage_abs="$(cd "$(dirname "$appimage")" && pwd)/$(basename "$appimage")"

chmod +x "$appimage_abs"
( cd "$work" && "$appimage_abs" --appimage-extract >/dev/null )
appdir="$work/squashfs-root"
[ -d "$appdir" ] || { echo "extraction produced no squashfs-root" >&2; exit 1; }

mapfile -t victims < <(find "$appdir/usr/lib" -maxdepth 2 -name 'libwayland-*.so*' | sort)
if [ "${#victims[@]}" -eq 0 ]; then
  # Not an error to swallow: if the bundler stops shipping them, this step is obsolete and should
  # be deleted rather than silently succeed forever.
  echo "::warning::no libwayland-* found in the AppDir; the bundler may have fixed tauri#15665 -- remove this step" >&2
  exit 0
fi
for victim in "${victims[@]}"; do
  echo "removing ${victim#"$appdir"/}"
  rm -f "$victim"
done

# Fail if anything Wayland-shaped survived, rather than shipping a half-fixed bundle.
if find "$appdir/usr/lib" -maxdepth 2 -name 'libwayland-*.so*' | grep -q .; then
  echo "libwayland-* still present after removal" >&2
  exit 1
fi

ARCH=x86_64 "$appimagetool" --no-appstream "$appdir" "$work/out.AppImage" >/dev/null
mv -f "$work/out.AppImage" "$appimage_abs"
chmod +x "$appimage_abs"
echo "repacked $(basename "$appimage_abs") without ${#victims[@]} libwayland files"
