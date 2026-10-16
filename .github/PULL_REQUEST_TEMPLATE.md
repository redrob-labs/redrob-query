<!-- Keep the title imperative and under 70 characters. -->

## What this changes

<!-- The behaviour that is different, not a restatement of the diff. -->

## Why

<!-- The problem. If it is a bug, say how it reproduced. -->

## How it was verified

<!-- The commands you actually ran, and what they printed. -->

```
python3 scripts/check-brand.py
cargo fmt --all -- --check
cargo clippy -p redrob-query -p redrob-query-core --all-targets -- -D warnings
cargo test -p redrob-query-core
```

## Checklist

- [ ] No credentials, connection strings, or real database contents in the diff,
      the tests, or the commit messages.
- [ ] Read-only guarantees preserved: typed queries still go through the engine's
      read-only gate, and writes only through reviewed edits and the structure forms.
- [ ] If local-storage behaviour changed, `docs/SECURITY.md` says what is now
      stored.
- [ ] If user-visible strings changed, they name Redrob Query — not DBeaver.
