<!-- Keep the title imperative and under 70 characters. -->

## What this changes

<!-- The behaviour that is different, not a restatement of the diff. -->

## Why

<!-- The problem. If it is a bug, say how it reproduced. -->

## How it was verified

<!-- The commands you actually ran, and what they printed. -->

```
npm run release:check
npm test
npm run build
npm run rust:fmt && npm run rust:clippy && npm run rust:test
```

## Checklist

- [ ] No credentials, connection strings, or real database contents in the diff,
      the tests, or the commit messages.
- [ ] Read-only guarantees preserved: no new desktop/native mutation command is
      registered, and saved desktop profiles stay read-only.
- [ ] If local-storage behaviour changed, `docs/SECURITY.md` says what is now
      stored.
- [ ] If user-visible strings changed, they name Redrob Data — not DBeaver.
