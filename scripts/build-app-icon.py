#!/usr/bin/env python3
"""Build the Redrob Query app icon master, then hand it to `tauri icon` for the platform sizes.

Geometry and rules are assets/Products/README.md, not invention:

  Tile        square top-left corner, 22% radius on the other three from 48px up; 19% at 32px.
  Symbol      the shipped file, Solid White, on every tile. Never redrawn or recoloured.
  Doorway     "Redrob's doorway is empty. Something in the doorway means a product." Query's is a
              three-tier store. Every shipped glyph is FILLED white with no stroke, and Desk's folder
              measures x 212..300, y 207..314, which is the room a glyph has.
  Perspective "Every row runs toward the door's vanishing point and slants less nearer the middle, as
              Office's rows do." So the three tiers narrow with depth rather than stacking square.
  Tile colour Code's band. 20-color.md closes the spectrum at seven and states there is no eighth
              product colour; 10-logo.md's way out is that a new product ships as a feature of the band
              it fits, and Query is a developer tool over a database. Pinned in DESIGN_SYSTEM_PIN.json.

The symbol path is LIFTED from a shipped product icon rather than retyped, so it cannot drift.
"""
import json
import pathlib
import re
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
DELIVERY = pathlib.Path.home() / "workplace/redrob-design-system/assets/Products"
MASTER = REPO / "src-tauri/icons/app-icon.svg"
SMALL = REPO / "src-tauri/icons/app-icon-small.svg"
PNG = REPO / "src-tauri/icons/app-icon.png"

pin = json.loads((REPO / "DESIGN_SYSTEM_PIN.json").read_text())
TILE_COLOUR = pin["product_band"]["tile"]

TILE = ("M0 0H399.36A112.64 112.64 0 0 1 512 112.64V399.36A112.64 112.64 0 0 1 399.36 512"
        "H112.64A112.64 112.64 0 0 1 0 399.36Z")
TILE_32 = ("M0 0H414.72A97.28 97.28 0 0 1 512 97.28V414.72A97.28 97.28 0 0 1 414.72 512"
           "H97.28A97.28 97.28 0 0 1 0 414.72Z")

source = DELIVERY / "redrob-code-icon.svg"
if not source.exists():
    sys.exit(f"design system delivery not found: {source}")
symbol = re.search(r'(<path d="M54\.4[^/]*?/>)', source.read_text())
if not symbol:
    sys.exit(f"could not lift the symbol path out of {source.name}")


def store() -> str:
    """Three tiers in the doorway, each narrower and shorter than the one above it."""
    tiers = []
    top, height, gap = 214.0, 26.0, 8.0
    for i in range(3):
        inset = i * 7.0
        left, right = 212.0 + inset, 300.0 - inset
        y = top + i * (height + gap)
        lip = (right - left) / 2 * 0.26  # the ellipse that gives the tier its lid
        tiers.append(
            f"M{left} {y}H{right}V{y + height - lip}"
            f"A{(right - left) / 2} {lip} 0 0 1 {left} {y + height - lip}Z"
        )
    return f'<path d="{"".join(tiers)}" fill="#FFFFFF" fill-rule="evenodd"/>'


MASTER.write_text(
    '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512"'
    ' role="img" aria-label="Redrob Query"><title>Redrob Query</title>'
    f'<path d="{TILE}" fill="{TILE_COLOUR}"/>{symbol.group(1)}{store()}</svg>\n'
)
print(f"wrote {MASTER.relative_to(REPO)}  tile {TILE_COLOUR}")

# "-32.svg and -16.svg carry tile and symbol only (the doorway is two or three pixels wide there)."
SMALL.write_text(
    '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512"'
    ' role="img" aria-label="Redrob Query"><title>Redrob Query</title>'
    f'<path d="{TILE_32}" fill="{TILE_COLOUR}"/>{symbol.group(1)}</svg>\n'
)
print(f"wrote {SMALL.relative_to(REPO)}  tile and symbol only, for 32px and below")

subprocess.run(
    ["magick", "-background", "none", str(MASTER), "-resize", "1024x1024", str(PNG)], check=True
)
subprocess.run(["npx", "tauri", "icon", str(PNG.relative_to(REPO))], cwd=REPO, check=True)

ICONS = REPO / "src-tauri/icons"
for name, size in (("32x32.png", 32), ("Square30x30Logo.png", 30), ("Square44x44Logo.png", 44)):
    subprocess.run(
        ["magick", "-background", "none", str(SMALL), "-resize", f"{size}x{size}",
         str(ICONS / name)],
        check=True,
    )
    print(f"re-rendered {name} from the small master")

ico_parts = []
for src, sizes in ((SMALL, (16, 24, 32, 48)), (MASTER, (64, 128, 256))):
    for s in sizes:
        out = ICONS / f".ico-{s}.png"
        subprocess.run(
            ["magick", "-background", "none", str(src), "-resize", f"{s}x{s}", str(out)], check=True
        )
        ico_parts.append(out)
subprocess.run(["magick", *map(str, ico_parts), str(ICONS / "icon.ico")], check=True)
for p in ico_parts:
    p.unlink()
print("rebuilt icon.ico: 16/24/32/48 without the doorway, 64/128/256 with it")
