# Changelog

The release workflow's notes point here, so this file is what a download's notes say. Versions
follow the `vMAJOR.MINOR.PATCH` tags that trigger a release; while the major is 0 a minor bump is
where behaviour may change.

## 0.2.2 - 2026-10-14

### Changed

- Desktop AI talks to the shared Redrob Code daemon (`<daemon>/v1/chat/completions`, with the
  daemon's authorization, `x-redrob-app: query` and a per-run `x-redrob-session`) and the daemon
  holds the Redrob console key. This app no longer carries a key of its own: `REDROB_API_KEY`, the
  keyring key read and the Redrob Launcher shared-account reader are gone (the engine reads the
  launcher's sign-in itself). The direct path to `console.redrob.ai` is removed, so masking applies
  in one place. **Redrob settings** still accepts a pasted key, but hands it to the engine
  (`PUT /auth/redrob`) and reads signed-in status from the engine (`GET /provider`), saying
  "Signed in with Redrob Launcher" when that is whose key it uses. A key an earlier version stored
  in the OS keyring is handed to the engine once and deleted. **Requires Redrob Code**: without it
  the settings dialog and every AI request say "Redrob Code is not installed. Install it with
  Redrob Launcher." with the launcher link; a start failure shows the engine's reason.
