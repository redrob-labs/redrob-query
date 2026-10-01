#!/usr/bin/env python3
"""Convert every colour literal in app.css to a design system token, by role.

This sheet is written one rule per LINE -- selector, braces and every declaration together -- so a
line-based pass sees no declarations at all. It is parsed by braces instead: the text is walked, the
current selector tracked, and each `prop: value;` rewritten wherever it sits.

Not a literal-for-literal table: the palette was improvised, so #a6aab5 has no token and never will.
What a token exists for is the ROLE -- body ink, a raised surface, a subtle border, a danger tint -- so
each site is classified by its property and the colour's own lightness and hue family.

Whole declarations first, because a per-colour pass gets these wrong: a gradient's stops are one
decision, not three; a shadow is a distance and a colour together, which the system ships as
`shadow-sm/md/lg`; and a `0 0 0 Npx` ring is a focus ring, not a shadow.

index.html carries data-theme="dark", so every block here sits on a dark ground and the ink ramp runs
the other way throughout.

20-color.md: "Accents never replace Redrob Blue as the brand signal; in product, prefer status-*." The
improvised violet and the brand red therefore land on the action colour, never on an accent step.
"""
from __future__ import annotations

import pathlib
import re
import sys

SRC = pathlib.Path("src/styles/app.css")
LITERAL = re.compile(r"#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)")


def srgb_to_linear(c: float) -> float:
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def parse(lit: str) -> tuple[float, float, float, float] | None:
    lit = lit.strip()
    if lit.startswith("#"):
        h = lit[1:]
        if len(h) == 3:
            h = "".join(c * 2 for c in h)
        if len(h) not in (6, 8):
            return None
        vals = [int(h[i : i + 2], 16) / 255 for i in range(0, 6, 2)]
        a = int(h[6:8], 16) / 255 if len(h) == 8 else 1.0
        return (*vals, a)  # type: ignore[return-value]
    m = re.match(r"rgba?\(([^)]*)\)", lit)
    if not m:
        return None
    nums = [p.strip() for p in m.group(1).replace("/", ",").split(",")]
    try:
        r, g, b = (float(n.rstrip("%")) / (100 if n.endswith("%") else 255) for n in nums[:3])
        a = float(nums[3]) if len(nums) > 3 else 1.0
    except ValueError:
        return None
    return r, g, b, a


def luminance(r: float, g: float, b: float) -> float:
    return 0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)


def family(r: float, g: float, b: float) -> str:
    mx, mn = max(r, g, b), min(r, g, b)
    if mx - mn < 0.10:
        return "neutral"
    if mx == r:
        h = 60 * (((g - b) / (mx - mn)) % 6)
    elif mx == g:
        h = 60 * ((b - r) / (mx - mn) + 2)
    else:
        h = 60 * ((r - g) / (mx - mn) + 4)
    if h < 18 or h >= 330:
        return "red"
    if h < 45:
        return "orange"
    if h < 70:
        return "amber"
    if h < 165:
        return "green"
    if h < 200:
        return "teal"
    if h < 255:
        return "blue"
    return "violet"


STATUS_INK = {"red": "--status-danger", "green": "--status-success",
              "orange": "--status-warning", "amber": "--status-warning",
              "blue": "--status-info", "teal": "--status-info", "violet": "--action-primary"}
FILL = {"red": "--action-primary", "violet": "--action-primary", "blue": "--action-primary",
        "teal": "--action-primary", "green": "--status-success",
        "orange": "--status-warning", "amber": "--status-warning"}
TINT = {"red": "--surface-brand-subtle", "green": "--accent-green-1",
        "orange": "--accent-orange-1", "amber": "--accent-yellow-1",
        "blue": "--accent-sky-1", "teal": "--accent-teal-1",
        "violet": "--surface-brand-subtle"}


def colour_token(prop: str, lit: str, focus: bool) -> str | None:
    rgba = parse(lit)
    if rgba is None:
        return None
    r, g, b, a = rgba
    lum, fam = luminance(r, g, b), family(r, g, b)
    # A hue thin enough to see through is decoration, not a state.
    if a < 0.35:
        fam = "neutral"

    if prop in ("color", "-webkit-text-fill-color", "fill", "stroke", "caret-color"):
        if fam != "neutral":
            return STATUS_INK[fam]
        # Dark ground: near-white is the body ink and the ramp descends from there.
        return ("--ink-primary" if lum > 0.62 else
                "--ink-secondary" if lum > 0.24 else "--ink-muted")

    if prop.startswith("border") or prop == "outline-color":
        if focus:
            return "--focus-ring"
        if fam != "neutral":
            return STATUS_INK[fam]
        return "--border-strong" if lum > 0.12 else "--border-subtle"

    if prop in ("background", "background-color"):
        if fam != "neutral":
            return FILL[fam] if lum < 0.45 else TINT[fam]
        if a < 0.9:
            return "--surface-material"
        return ("--surface-base" if lum < 0.012 else
                "--surface-raised" if lum < 0.020 else
                "--surface-sunken" if lum < 0.035 else "--border-subtle")
    return None


def shadow_value(value: str) -> str:
    out: list[str] = []
    for layer in re.split(r",(?![^(]*\))", value):
        layer = layer.strip()
        if not LITERAL.search(layer):
            out.append(layer)
            continue
        if re.match(r"^(inset\s+)?0\s+0\s+0\s+[\d.]+", layer):
            out.append("0 0 0 var(--focus-ring-width) var(--focus-ring)")
            continue
        blurs = [float(n) for n in re.findall(r"([\d.]+)px", layer)]
        blur = max(blurs) if blurs else 0
        token = "--shadow-sm" if blur < 12 else "--shadow-md" if blur < 45 else "--shadow-lg"
        out.append(f"var({token})")
    kept: list[str] = []
    for item in out:
        if item.startswith("var(--shadow") and item in kept:
            continue
        kept.append(item)
    return ", ".join(kept)


def gradient_value(value: str) -> str:
    stops = [s for s in (parse(m.group(0)) for m in LITERAL.finditer(value)) if s]
    solid = [s for s in stops if s[3] > 0.5]
    if not solid:
        return "var(--wash-deep)"
    lums = [luminance(*s[:3]) for s in solid]
    fams = {family(*s[:3]) for s in solid}
    if fams <= {"neutral"}:
        # A neutral gradient on a dark ground is a surface step, and data-theme already supplies it.
        return "var(--surface-raised)" if max(lums) < 0.12 else "var(--surface-sunken)"
    return "var(--gradient-deep)"


text = SRC.read_text()
out: list[str] = []
selector = ""
pos = 0
converted = 0
notes: list[str] = []

DECL = re.compile(r"([a-z-]+)\s*:\s*([^;{}]*?)\s*(?=[;}])")

# Walk the file in rule-sized pieces so a selector is known for every declaration, whatever the layout.
for match in re.finditer(r"([^{}]*)\{([^{}]*)\}", text, re.S):
    head, body = match.group(1), match.group(2)
    out.append(text[pos : match.start()])
    pos = match.end()
    sel = head.strip().split("\n")[-1].strip()
    if sel:
        selector = sel
    focus = ":focus" in selector
    new_body = body

    for decl in list(DECL.finditer(body)):
        prop, value = decl.group(1), decl.group(2)
        if prop.startswith("--") or not LITERAL.search(value):
            continue
        if "shadow" in prop:
            replacement = shadow_value(value)
        elif "gradient(" in value:
            replacement = gradient_value(value)
        elif prop == "outline":
            replacement = LITERAL.sub("var(--focus-ring)", value)
            replacement = re.sub(r"[\d.]+px", "var(--focus-ring-width)", replacement, count=1)
        else:
            def repl(mm: re.Match[str], _p: str = prop) -> str:
                global converted
                tok = colour_token(_p, mm.group(0), focus)
                if tok is None:
                    notes.append(f"{_p:20} {mm.group(0):24} {selector[:44]}")
                    return mm.group(0)
                converted += 1
                return f"var({tok})"

            replacement = LITERAL.sub(repl, value)
        if replacement != value:
            if "shadow" in prop or "gradient(" in value or prop == "outline":
                converted += len(LITERAL.findall(value))
            new_body = new_body.replace(f"{prop}: {value}", f"{prop}: {replacement}", 1)
            new_body = new_body.replace(f"{prop}:{value}", f"{prop}: {replacement}", 1)

    out.append(head + "{" + new_body + "}")

out.append(text[pos:])
SRC.write_text("".join(out))

print(f"converted {converted} colour sites to tokens")
left = LITERAL.findall(SRC.read_text())
print(f"{len(left)} literals remain")
for n in dict.fromkeys(notes):
    print("  " + n)
if left:
    for m in re.finditer(r".*(?:#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)).*", SRC.read_text()):
        print("  still:", m.group(0).strip()[:120])
        if len(left) > 12:
            break
sys.exit(0)
