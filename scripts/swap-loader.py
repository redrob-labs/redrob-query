#!/usr/bin/env python3
"""Replace the hand-drawn spinner with the design system's Loader, and the letter-R with the mark.

The spinner was a bordered div spun by a keyframe. The delivery ships `Loader` for "something is
happening and the share is unknown", and it keeps its label in the DOM, visible or screen-reader only:
a bare spinning border tells a screen reader nothing, so the page looks broken to anyone who cannot see
it move. The label comes from the sentence each site already prints beside it.

The boot screen drew a letter R in a tile. 10-logo.md: the artwork is "Never redrawn, restretched,
recolored or otherwise modified", and a typed R is not a modification of the mark, it is a different
mark. Both grounds are vendored and pinned in DESIGN_SYSTEM_PIN.json.
"""
import pathlib
import sys

# (file, exact fragment, replacement, why)
EDITS = [
    (
        "src/App.tsx",
        '<div className="app-loading"><div className="loading-logo">R</div>'
        '<span className="spinner" /><p>Opening your data workspace…</p></div>',
        '<div className="app-loading"><Mark src={symbolLight} darkSrc={symbolDark} height={40} alt="" />'
        '<Loader size="md" label="Opening your data workspace" /><p>Opening your data workspace…</p></div>',
        "boot screen: the shipped mark and the shipped loader",
    ),
    (
        "src/components/AiSettingsModal.tsx",
        '<span className="spinner small" />',
        '<Loader size="sm" label="Saving the key" />',
        "saving the AI key",
    ),
    (
        "src/components/ChangesPanel.tsx",
        '<span className="spinner small" />',
        '<Loader size="sm" label="Applying staged changes" />',
        "applying staged changes",
    ),
    (
        "src/components/Navigator.tsx",
        '<span className="spinner" /> Connecting…',
        '<Loader size="sm" label="Connecting" /> Connecting…',
        "connecting to the database",
    ),
    (
        "src/components/Navigator.tsx",
        '<span className="spinner" /> Loading schema…',
        '<Loader size="sm" label="Loading the schema" /> Loading schema…',
        "loading the schema",
    ),
    (
        "src/components/Navigator.tsx",
        '<span className="spinner small" />',
        '<Loader size="sm" label="Removing the connection" />',
        "removing a connection",
    ),
    (
        "src/components/QueryEditor.tsx",
        '<span className="spinner small" />',
        '<Loader size="sm" label="Running the query" />',
        "running a query, in the toolbar button",
    ),
    (
        "src/components/ResultGrid.tsx",
        '<span className="spinner" />',
        '<Loader size="md" label="Loading rows" />',
        "loading result rows",
    ),
]

for path_str, old, new, why in EDITS:
    path = pathlib.Path(path_str)
    text = path.read_text()
    if old not in text:
        sys.exit(f"{path_str}: fragment absent ({why})\n  {old[:90]}")
    text = text.replace(old, new, 1)
    path.write_text(text)
    print(f"  {path_str}: {why}")

# ConnectionModal has two: the Test button and the Save button, in that order on one line.
path = pathlib.Path("src/components/ConnectionModal.tsx")
text = path.read_text()
assert text.count('<span className="spinner small" />') == 2, "expected exactly two spinners"
text = text.replace(
    '<span className="spinner small" />', '<Loader size="sm" label="Testing the connection" />', 1
)
text = text.replace(
    '<span className="spinner small" />', '<Loader size="sm" label="Saving the connection" />', 1
)
path.write_text(text)
print("  src/components/ConnectionModal.tsx: test, then save")

# QueryEditor's remaining full-size spinner is the editor's own loading state.
path = pathlib.Path("src/components/QueryEditor.tsx")
text = path.read_text()
if '<span className="spinner" />' in text:
    text = text.replace(
        '<span className="spinner" />', '<Loader size="md" label="Loading the editor" />', 1
    )
    path.write_text(text)
    print("  src/components/QueryEditor.tsx: editor loading state")

left = [
    str(p)
    for p in pathlib.Path("src").rglob("*.tsx")
    if 'className="spinner' in p.read_text()
]
print("\nfiles still drawing a spinner:", left or "none")
