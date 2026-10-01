#!/usr/bin/env python3
"""Prove the guard fails when the thing it guards is broken.

A guard that has never failed is a guard nobody has tested, and a green suite is exactly what a broken
guard produces. Each defect is put back, the guard is run, and the file is restored from bytes held in
memory -- not from git, because none of this is committed yet and a checkout would delete the work.
"""
import pathlib
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
GUARD = ["node", "scripts/check-design-system.mjs"]

CASES = [
    ("src/styles/app.css", lambda t: t + "\n.guard-probe { color: #ff0000; }\n", "raw colour literal"),
    ("src/ui/Icon.tsx", lambda t: 'import { X } from "lucide-react";\n' + t, "lucide import"),
    ("src/styles/app.css", lambda t: t + '\n.guard-probe { font-family: "Comic Sans MS"; }\n',
     "off-system typeface"),
    ("src/assets/brand/redrob-symbol.png", None, "edited brand artwork"),
]

failed = []
for path, mutate, label in CASES:
    target = REPO / path
    original = target.read_bytes()
    try:
        if mutate is None:
            target.write_bytes(original + b"\x00")  # one byte is enough to change the hash
        else:
            target.write_text(mutate(original.decode()))
        result = subprocess.run(GUARD, cwd=REPO, capture_output=True, text=True)
        if result.returncode == 0:
            failed.append(f"{label}: guard PASSED with the defect present")
        else:
            first = next((l.strip() for l in result.stderr.splitlines() if l.strip().startswith(("src/", "package.json"))), "")
            print(f"  caught  {label}\n            {first[:110]}")
    finally:
        target.write_bytes(original)

clean = subprocess.run(GUARD, cwd=REPO, capture_output=True, text=True)
if clean.returncode != 0:
    failed.append("the tree did not restore cleanly after the probes:\n" + clean.stderr)

if failed:
    print("\nreverse verification FAILED:")
    for f in failed:
        print("  " + f)
    sys.exit(1)
print("\nall four checks fail on their defect, and the tree restored clean")
