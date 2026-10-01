#!/usr/bin/env python3
"""Tell an error apart from the old brand red, which the classifier cannot.

Both were #ff405c, so classifying by hue sent every one of them to `status-danger`. That makes the DEMO
badge, the Redrob AI mark, the bot avatar and the REDROB DATA label in the status bar all render in the
colour this app uses for failure -- a workspace that looks broken while nothing is wrong.

A site keeps `status-danger` only when its own selector says it is a failure or a destructive action.
Everything else was the brand, and 20-color.md sends the brand signal to Redrob Blue: `ink-brand` where
it is ink on dark, which is `blue-4` and reads at text size, not `action-primary`, which is a fill.
"""
import pathlib
import re
import sys

SRC = pathlib.Path("src/styles/app.css")

# A failure, or an action that destroys something. These keep the danger colour.
KEEP = (
    "error", "danger", "invalid", "del", "remove-confirmation",
    "change-card > button:hover", "profile-actions-menu button.danger",
)

text = SRC.read_text()
lines = text.split("\n")
changed = kept = 0
report: list[str] = []

for i, line in enumerate(lines):
    if "var(--status-danger)" not in line:
        continue
    # One rule per line in this sheet, so the selector is whatever precedes the first brace.
    for rule in re.finditer(r"([^{}]+)\{([^{}]*)\}", line):
        selector, body = rule.group(1).strip(), rule.group(2)
        if "var(--status-danger)" not in body:
            continue
        if any(k in selector for k in KEEP):
            kept += 1
            report.append(f"  keep  {selector[:70]}")
            continue
        new_body = body.replace("var(--status-danger)", "var(--ink-brand)")
        lines[i] = lines[i].replace(rule.group(0), selector + " {" + new_body + "}", 1)
        changed += 1
        report.append(f"  brand {selector[:70]}")

SRC.write_text("\n".join(lines))
for r in report:
    print(r)
print(f"\n{changed} site(s) were the brand, {kept} are genuinely a failure or destructive")

# A border in the danger colour on the PRIMARY button was never right either: the fill is the action
# colour, so its edge cannot be the failure colour.
text = SRC.read_text()
before = text
text = text.replace(
    ".primary-button { border-color: var(--ink-brand); background: var(--accent); color: white;",
    ".primary-button { border-color: var(--action-primary); background: var(--action-primary);"
    " color: var(--ink-on-brand);",
)
text = text.replace(
    ".danger-button { border-color: var(--status-danger); background: var(--action-primary);"
    " color: white;",
    ".danger-button { border-color: var(--status-danger); background: var(--status-danger);"
    " color: var(--ink-on-brand);",
)
if text == before:
    sys.exit("neither button rule matched -- check the selectors before trusting this script")
SRC.write_text(text)
print("primary button: action fill and edge; danger button: danger fill; both take ink-on-brand")
print("remaining literal 'white' keywords:", len(re.findall(r":\s*white\b", text)))
