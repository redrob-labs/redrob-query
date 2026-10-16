#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Stage one platform's release tree from the built `redrob-query` binary.

    stage.py --target <rust-triple> --version X.Y.Z --out <dir> [--binary <path>]

The tree every archive, package and installer is made from (release.yml, build-linux.sh):

  Linux / Windows                        macOS
    bin/redrob-query[.exe]                Redrob Query.app/
    share/redrob-query/licenses/            Contents/Info.plist
      LICENSE  NOTICE                        Contents/MacOS/redrob-query
      UPSTREAM_NOTICES.md                    Contents/Resources/AppIcon.icns
                                             Contents/Resources/licenses/...
                                           share/redrob-query/licenses/   (the same set, for the dmg)

The licence texts travel with every copy (GPL-3.0-or-later section 4 asks the notices to be kept with
every conveyed copy), so they are inside the bundle as well as beside it.
The binary defaults to target/<target>/release/redrob-query[.exe]; --binary overrides it.

The Info.plist is written here rather than committed so the version in it is the workspace version
the build used; release.yml reads it back and refuses a mismatch.
"""
from __future__ import annotations

import argparse
import plistlib
import re
import shutil
import stat
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LICENSES = (
    "LICENSE",
    "NOTICE",
    "UPSTREAM_NOTICES.md",
)
ICNS = ROOT / "resources" / "icons" / "redrob-query.icns"
BUNDLE_ID = "ai.redrob.query"
APP_NAME = "Redrob Query"
BINARY = "redrob-query"


def copy_licenses(into: Path) -> None:
    into.mkdir(parents=True, exist_ok=True)
    for name in LICENSES:
        source = ROOT / name
        if not source.is_file():
            sys.exit(f"missing {source}; the licence set must be complete before staging")
        shutil.copyfile(source, into / name)


def executable(path: Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def stage_unix_or_windows(binary: Path, out: Path, windows: bool) -> None:
    bin_dir = out / "bin"
    bin_dir.mkdir(parents=True)
    dest = bin_dir / (f"{BINARY}.exe" if windows else BINARY)
    shutil.copyfile(binary, dest)
    if not windows:
        executable(dest)
    copy_licenses(out / "share" / "redrob-query" / "licenses")


def stage_macos(binary: Path, out: Path, version: str) -> None:
    app = out / f"{APP_NAME}.app"
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Resources").mkdir(parents=True)
    dest = contents / "MacOS" / BINARY
    shutil.copyfile(binary, dest)
    executable(dest)
    if not ICNS.is_file():
        sys.exit(f"missing {ICNS}; run scripts/build-brand-assets.py")
    shutil.copyfile(ICNS, contents / "Resources" / "AppIcon.icns")
    copy_licenses(contents / "Resources" / "licenses")
    copy_licenses(out / "share" / "redrob-query" / "licenses")
    info = {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleDisplayName": APP_NAME,
        "CFBundleExecutable": BINARY,
        "CFBundleIconFile": "AppIcon",
        "CFBundleIdentifier": BUNDLE_ID,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": APP_NAME,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "12.0",
        "LSApplicationCategoryType": "public.app-category.developer-tools",
        "NSHighResolutionCapable": True,
        "NSHumanReadableCopyright": "Copyright 2026 Janghoon Lee (Redrob) and contributors. GPL-3.0-or-later.",
        "NSSupportsAutomaticGraphicsSwitching": True,
    }
    with open(contents / "Info.plist", "wb") as handle:
        plistlib.dump(info, handle, sort_keys=True)
    (contents / "PkgInfo").write_text("APPL????", encoding="ascii")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--target", required=True, help="the Rust target triple that was built")
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--binary", type=Path, help="the built binary (default target/<target>/release/)")
    args = parser.parse_args()

    if not re.fullmatch(r"\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", args.version):
        sys.exit(f"--version must be X.Y.Z, got {args.version!r}")
    windows = "windows" in args.target
    macos = "apple-darwin" in args.target
    binary = args.binary or (
        ROOT / "target" / args.target / "release" / (f"{BINARY}.exe" if windows else BINARY)
    )
    if not binary.is_file():
        sys.exit(f"binary not found: {binary}; run cargo build --release -p redrob-query --target {args.target}")

    out = args.out.resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    if macos:
        stage_macos(binary, out, args.version)
    else:
        stage_unix_or_windows(binary, out, windows)
    print(f"staged {args.target} {args.version} into {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
