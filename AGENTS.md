# AGENTS.md

Operating notes for an agent working in `redrob-labs/redrob-query`. Everything here was read from
the tree or measured against the live repository, not assumed.

## What this repository is

Redrob Query is a native desktop database client for PostgreSQL, MySQL, SQLite and MongoDB: a Rust
egui/wgpu shell (`apps/redrob-query`) over a Rust engine crate (`crates/redrob-query-core`). Version
0.3.0 replaced the Tauri 2 + React 19 shell outright; there is no TypeScript, no Node, no browser
demo and no plugin host any more. The shape of the repository follows `redrob-labs/redrob-canvas`.

The workflow is informed by DBeaver Community and parts of the engine were ported from Beekeeper
Studio Community Edition (GPL-3.0-or-later, which is why this tree is GPL). There is no DBeaver,
Eclipse, OSGi, JDBC or Java code here. Keep it that way: user-visible strings name Redrob Query.

## Layout

| Path | What lives there |
| --- | --- |
| `apps/redrob-query/` | The shell. `main.rs` (window, icon, `--version`), `app.rs` (panels and event routing), `profiles.rs` (tree + connection form), `schema.rs` (lazy schema browser), `editor.rs` (tabs, run / format / save), `highlight.rs` (SQL/MQL lexer, formatter, starters), `grid.rs` (virtualised result grid, staging, exports), `changes.rs` (staged edits -> `CellEditSet`), `structure.rs` (keys/indexes + DDL forms), `mongo.rs` (request strip), `ai_dock.rs` (Ask), `account.rs` (shared account watcher), `workspace.rs` (tabs/history persistence), `services.rs` (config dir), `theme.rs` (the only colour literals). `build.rs` embeds the Windows icon and version info. |
| `crates/redrob-query-core/` | The engine: `connection.rs` (`DataService`), `query.rs` (read-only classification), `models.rs`, `profile_store.rs`, `secret.rs` (OS keyring), `cell_edits.rs`, `ddl.rs`, `keys.rs` (PK/FK/index readers), `account.rs` (the launcher's shared `account.json`), `ai.rs` (console-direct chat), `workspace.rs` (the `Workspace` facade the shell drives, one method per former Tauri command, answered through `Pending<T>`). `publish = false`. |
| `brand/SOURCES.json` | sha256 pins of every brand asset against the design system; written by `scripts/build-brand-assets.py`, checked by `scripts/check-brand.py`. Never edit by hand. |
| `resources/icons`, `resources/brand` | Generated brand assets (hicolor PNGs, `.ico`, `.icns`, SVGs, lockups, splash). Owned by the brand pipeline. |
| `installer/windows/` | The bilingual (English / 한국어) Win32 setup and uninstaller, ported from redrob-launcher; `build.py` compiles it on a Windows runner. `installer/macos/` holds the dmg settings and the uninstaller script. |
| `tools/release/` | `stage.py` (per-platform tree: `bin/`, licences, the `.app`), `make_deb.py`; `tools/release_matrix.py` is the one list of the three targets. |
| `docs/` | `ARCHITECTURE.md`, `SECURITY.md`, `DBEAVER-PORTING-MAP.md`, `compatibility.md`, `dialect-verification.md`, `beekeeper-porting-boundary.md`, `upstream-sources.toml`. Some of these still describe the Tauri shell's paths; the engine descriptions hold. |
| `NOTICE`, `UPSTREAM_NOTICES.md`, `LICENSE` | Attribution. Upstream lines are never replaced, only added to. |

Documentation is bilingual: `README.md` and `CONTRIBUTING.md` have `.ko.md` siblings. Change both or
neither.

## Toolchain and commands

Rust comes from rustup; `rust-toolchain.toml` pins channel `1.95.0` with `clippy` and `rustfmt`, and
`rust-version` in `Cargo.toml` matches it. The workspace members are `crates/*` and `apps/*`. On Linux
the shell needs `libxkbcommon-dev libwayland-dev libx11-dev libxrandr-dev libxi-dev libgl1-mesa-dev
libgtk-3-dev libdbus-1-dev`. Run desktop binaries on a display; `redrob-query --version` needs none.

The gate, as CI runs it (`.github/workflows/ci.yml`, pull requests only):

```bash
python3 scripts/check-brand.py
cargo fmt --all -- --check
cargo clippy -p redrob-query -p redrob-query-core --all-targets --locked --profile ci -- -D warnings
cargo test -p redrob-query -p redrob-query-core --locked --profile ci
```

`--profile ci` compiles every dependency at opt-level 0 (root `Cargo.toml`); the `dev` profile
optimises dependencies at level 2, which makes a cold test run take far longer. Clippy runs with
`all` and `pedantic` warned and `-D warnings`, so a pedantic lint fails the build. `unsafe_code` is
forbidden workspace-wide. The release workflow repeats only `cargo fmt --check` and the brand check.

Three connector tests in the core are `#[ignore]` and need live databases
(`REDROB_TEST_POSTGRES_URL`, `REDROB_TEST_MYSQL_URL`, `REDROB_TEST_MONGO_URL`). Skipping them is
normal; claiming they passed without exporting the URLs is not.

## Branch model

`develop` is the default branch and the base of every pull request. `main` is released state and
moves only by a pull request from `develop` (a `release/<version>` bump lands on `develop` first).
Release tags are cut from `main`; the release workflow refuses a tag that is not reachable from
`origin/main`, that does not equal the workspace version, or whose version has no `## <version>`
section in `CHANGELOG.md`. After a release, `main` is merged back into `develop`. Check before you
trust it:

```bash
git rev-list --count origin/develop..origin/main   # commits on main that develop lacks
git rev-list --count origin/main..origin/develop   # commits on develop that main lacks
```

Working branches are `<type>/<slug>` off `develop`. Merges into `develop` are squash merges with an
explicit commit message; the branch is deleted afterwards. PR policy is the organisation's reusable
workflow (`.github/workflows/pr-policy.yml`).

## What an agent gets wrong first

- **Brand assets are generated and pinned.** Editing anything under `resources/icons`,
  `resources/brand` or `installer/*/res/icon.png|lockup.png` by hand fails `check-brand.py`. Rebuild
  with `scripts/build-brand-assets.py --design-system <redrob-design-system checkout>` (needs
  ImageMagick 7) and commit `brand/SOURCES.json` with the outputs.
- **The read-only guarantee is the engine's gate, not the shell's.** `DataService::execute_query`
  accepts read-only statements only; writes go through `apply_cell_edits` (one transaction, every
  statement must change exactly one row) and `apply_table_change` (a fixed type list). Do not add a
  write path that bypasses them.
- **No sign-in UI.** The account is Redrob Launcher's shared `account.json`
  (`crates/redrob-query-core/src/account.rs`): read on every request, never written, never logged or
  shown. `REDROB_API_KEY` and this app's own `account.json` win over it. Do not store a key in the
  keyring or print one. The one allowed action is opening the launcher's page.
- **The AI path is console-direct.** `ai.rs` posts to `{apiBaseUrl}/chat/completions` with a bearer
  key and model `auto` and surfaces the console's `error.message`. There is no Redrob Code daemon in
  between any more; do not reintroduce `redrob-engine-client`.
- **Colour literals live in `theme.rs` only**, named after their design-system tokens. The shell
  otherwise paints from egui's theme.
- **Workspace persistence is bounded.** 30 tabs, 50 history entries, query text only, written
  atomically to `workspace.json`; `REDROB_QUERY_NO_PREFS` turns it off. Never persist result rows,
  passwords or prompts.
- **Release assets must keep the launcher's names**: `redrob-query-<version>-<os>-<arch>[-portable|-setup].<ext>`
  from `tools/release_matrix.py`. The Windows installed product is ProductName `Redrob Query`,
  executable `bin\redrob-query.exe`, Settings > Apps key `RedrobQuery`.
- **GitHub Actions are SHA-pinned** in the workflows. Keep the `# vX.Y.Z` comment next to the SHA.
- **Do not commit secrets.** Database passwords live in the OS keyring; nothing in the tree should
  hold one.
