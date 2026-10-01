#!/usr/bin/env python3
"""Swap Query's lucide glyphs for the design system's, across all eleven files that use them.

Mappings that are not a plain rename, and why:

  Bot, Sparkles, WandSparkles -> sparkle   45-icons.md gives the AI group two glyphs and `sparkle` is
                                           the one for generated content, which is what all three mark.
  Code2, Braces               -> code       one meaning, one icon.
  DatabaseZap                 -> database   it labels the connection chip; the set has no zap variant
                                           and "database, energised" is not a second meaning.
  TestTube2                   -> pulse      the button reads "Test connection" -- a liveness reading.
  GitCompareArrows            -> compare
  Table2 -> grid, TableProperties -> layout, Columns3 -> columns
  Circle -> dot                            45-icons.md: a dot is a 2x2 filled square, and `dot` is the
                                           glyph for it; an outline circle meant nothing here.
  Link2Off -> plug                         the control is "Disconnect"; the label carries the off-state.
  TextCursorInput -> note, ToggleLeft -> sliders   column data types, see TYPE_ICON below.
  AlertTriangle -> warning, CheckCircle2 -> circleCheck, XCircle -> circleX
"""
from __future__ import annotations

import pathlib
import re
import sys

MAP = {
    "AlertTriangle": "warning", "ArrowDown": "arrowDown", "ArrowRight": "arrowRight",
    "ArrowUp": "arrowUp", "Bot": "sparkle", "Braces": "code", "Check": "check",
    "CheckCircle2": "circleCheck", "ChevronDown": "chevronDown", "ChevronRight": "chevronRight",
    "Circle": "dot", "Clock3": "clock", "CloudOff": "cloudOff", "Code2": "code",
    "Columns3": "columns", "Copy": "copy", "Database": "database", "DatabaseZap": "database",
    "Download": "download", "Edit3": "edit", "Eye": "eye", "File": "file",
    "FileCode2": "fileCode", "FilePlus2": "filePlus", "GitCompareArrows": "compare",
    "Hash": "hash", "Info": "info", "KeyRound": "key", "Keyboard": "keyboard",
    "Lightbulb": "lightbulb", "Link": "link", "Link2Off": "plug", "LockKeyhole": "lock",
    "MoreHorizontal": "more", "PanelRightClose": "panelRight", "Play": "play", "Plus": "plus",
    "RefreshCw": "refresh", "Search": "search", "Server": "server", "Settings2": "settings",
    "ShieldCheck": "shieldCheck", "Sparkles": "sparkle", "Table2": "grid",
    "TableProperties": "layout", "TestTube2": "pulse", "TextCursorInput": "note",
    "ToggleLeft": "sliders", "Trash2": "trash", "WandSparkles": "sparkle", "X": "close",
    "XCircle": "circleX",
}

FILES = sorted(pathlib.Path("src").rglob("*.tsx"))
total = 0
touched = []

for path in FILES:
    text = original = path.read_text()
    if "lucide-react" not in text:
        continue

    # The import block goes; the wrapper comes in, on the line the old import held so the order of
    # imports does not churn.
    text, n = re.subn(
        r"import \{[^}]*\} from '(?:lucide-react)';\n",
        "import { Icon } from '@ui/Icon';\n",
        text,
        count=1,
    )
    if n != 1:
        sys.exit(f"{path}: could not replace the lucide import")

    def rewriter(glyph: str):
        def one(m: re.Match[str]) -> str:
            attrs = m.group(1) or ""
            px = re.search(r"size=\{(\d+)\}", attrs)
            rest = re.sub(r"\s*size=\{\d+\}", "", attrs).rstrip()
            # 16 beside body or label, 24 beside body-lg. Nothing between, nothing outside.
            size = " size={24}" if px and int(px.group(1)) > 16 else ""
            return f'<Icon name="{glyph}"{size}{rest} />'

        return one

    for lucide, glyph in MAP.items():
        text, k = re.subn(rf"<{lucide}((?:\s+[^<>]*?)?)\s*/>", rewriter(glyph), text)
        total += k

    if text != original:
        path.write_text(text)
        touched.append(str(path))

print(f"rewrote {total} glyph sites across {len(touched)} files")
leftover = []
for path in FILES:
    text = path.read_text()
    for name in MAP:
        if re.search(rf"<{name}[\s/>]", text) or re.search(rf"\b{name}\b(?!\w)", text) and "lucide" in text:
            leftover.append(f"{path}: {name}")
if leftover:
    print("\nstill referencing a lucide component (expected for icons passed as values):")
    for item in sorted(set(leftover)):
        print("  " + item)
