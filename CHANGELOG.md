# Changelog

The release workflow's notes point here, so this file is what a download's notes say. Versions
follow the `vMAJOR.MINOR.PATCH` tags that trigger a release; while the major is 0 a minor bump is
where behaviour may change.

## 0.5.0 - 2026-10-17

### Added

- **Control from Redrob Code.** While the app is open it serves its tools to the shared Redrob
  Code daemon over loopback MCP as `app:query` (redrob-code v0.7.0 app-bridge protocol), so the
  agent in any Redrob surface can drive it: `list_connections`, `connect`, `list_tables`,
  `describe_table`, `run_sql` (one read-only statement, TSV, 200 rows by default), `open_in_editor`
  and `export_result` (CSV / TSV). Every tool goes through the same engine paths the panels use, so
  a bridged query is bounded and gated exactly like one typed into a tab; the daemon is optional and
  without one the app runs as before.

## 0.4.0 - 2026-10-16

### Changed

- **The window is laid out like Redrob Canvas.** One theme owned by the shell (canvas's Pro
  tokens, dark and light grounds, following the system with a Help > Theme override; Inter for the
  interface, JetBrains Mono for SQL and grid cells, a system CJK fallback), a thin top bar with File
  / Connection / Query / View / Help menus and a right slot showing the active connection with its
  read-only / read-write badge beside the Redrob account, a left panel with the connections tree
  (grouped by database, connect-state dots, filter) and the active connection's schema tree
  (database > schema > tables > columns with dimmed types, filter; double-click inserts the name;
  Select top 100 / Copy name / DDL on right-click), tabbed editors (Ctrl+T, Ctrl+W, Ctrl+1…9; a
  tab shows its connection and a dirty dot) with the result area under a splitter as Grid /
  Messages / Changes tabs, the Ask dock on the right (Ctrl+Shift+A), and a status line at the
  bottom. Every string is in English and Korean (Help > Language).
- **Running.** Ctrl+Enter runs the selection or the statement under the cursor, F5 the whole
  text, Esc cancels with a spinner on the status line (`Pending::cancel` in the engine). Errors
  go to the Messages tab and the red status line; there is no error dialog.
- **Grid.** Monospace cells, header click sorts, column edges drag, click selects a cell and
  Ctrl+C copies it (Ctrl+click several rows, Ctrl+C copies them as TSV), NULL dimmed italic,
  long text ellipsized with the full value as a tooltip, row range and page controls at the bottom.
- **Reviewed edits** stage in the grid, count on the Changes tab's badge, and show the exact SQL
  the engine will run (bound values beside it) before Apply or Discard.
- **First run** opens on an empty state with one action, New connection; the connection form is
  a modal with a Test button and inline error text.
- **View > History** lists past runs; click inserts into the editor, double-click opens a tab.
  Help > Shortcuts lists the keys.
- **Release manifest.** `redrob-launcher.json` names the launcher icon `redrob-query.png`,
  uploaded as one more release asset.

## 0.3.0 - 2026-10-16

### Changed

- **Redrob Query is a native Rust app.** The Tauri 2 + React 19 shell, the browser demo, the
  Beekeeper plugin host and the ER-diagram plugin are gone; the window is egui 0.36 on wgpu
  (`apps/redrob-query`) over the same engine, renamed `redrob-query-core`. Profiles, schema browser,
  tabbed SQL / Mongo JSON editor (colouring, Ctrl+Enter, Ctrl+Shift+F, Ctrl+S, starter queries,
  read-only batches), virtualised result grid (page sort / filter, column visibility, TSV / JSON
  export, JSON document view), reviewed cell edits and row deletes through the Changes panel,
  structure panel with primary and foreign keys and indexes, create-table / add-column forms, and
  the Ask dock are all native. Workspace persistence keeps 30 tabs and 50 history entries in
  `workspace.json` (off with `REDROB_QUERY_NO_PREFS`).
- **Ask talks to the Redrob console directly** with the account Redrob Launcher signed this
  computer in with (`account.json`, read on every request and watched while the app is open), as
  the launcher's shared-account contract asks. The Redrob Code daemon path, the pasted-key dialog
  and the keyring key migration are gone; the console's own error message is shown when it refuses.
- **Key and index metadata moved into the engine** (`keys.rs`): PostgreSQL catalog, MySQL
  `information_schema`, SQLite table-valued pragmas, MongoDB index list.
- **Release and repository shape follow redrob-canvas.** One `Release` workflow builds the three
  targets from the cargo binary -- Linux tar.gz + deb, macOS arm64 dmg + zip (signed, notarized,
  stapled), Windows x64 portable zip + a bilingual (English / 한국어) setup (Authenticode-signed) --
  with `SHA256SUMS.txt` and `redrob-launcher.json` from the shared publish job. Brand assets are
  built from the design system and pinned in `brand/SOURCES.json`. CI is one thin pull-request
  workflow: the brand check plus fmt, clippy and the tests of our crates with `--profile ci`.
- The Tauri updater feed (`latest.json`) is no longer produced; Redrob Launcher installs from the
  release assets.

## 0.2.4 - 2026-10-14

### Fixed

- rustls updated to 0.23.46 for RUSTSEC-2026-0347 and RUSTSEC-2026-0348, which the release
  gate's cargo-audit refused on v0.2.3. No application change.

## 0.2.3 - 2026-10-14

### Fixed

- The Linux release step that repacks and re-signs the AppImage looked the asset up by its local
  file name, but GitHub stores an uploaded asset with every space turned into a dot, so the step
  failed on every tag since it was added (v0.2.1, v0.2.2) and those releases stayed drafts. It now
  searches for the stored name. No application change.

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
