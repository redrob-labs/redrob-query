# Redrob Query

**English** · [한국어](./README.ko.md)

Redrob Query is an agentic database client for PostgreSQL, MySQL, SQLite and MongoDB: one native
window (Rust, egui on wgpu) that reads by default, edits only where you allow it after you have
reviewed the change, and drafts queries with an agent beside you. There is no JVM, no browser
runtime and no web demo. Signing in happens once, in [Redrob Launcher](https://github.com/redrob-labs/redrob-launcher);
this app reads that account.

The workflow is informed by DBeaver Community and parts of the engine were ported from Beekeeper
Studio Community Edition (see [NOTICE](NOTICE) and [UPSTREAM_NOTICES.md](UPSTREAM_NOTICES.md)). It
includes no Eclipse RCP, OSGi, JDBC, Java or Electron.

## What ships

| Area | Capability |
|---|---|
| Profiles | Built-in **Demo SQLite** (`:memory:`, works offline) plus your own profiles: create, test, edit, connect, disconnect, remove. Passwords live in the OS keyring, never in a file |
| PostgreSQL / MySQL | Identity-verifying TLS, schema-grouped tables and views, columns, primary and foreign keys, indexes, bounded read-only SQL, reviewed edits |
| SQLite | File profiles (read-only `mode=ro` until edits are allowed), tables, columns, keys and indexes through the table-valued pragmas, reviewed edits |
| MongoDB | Standard and SRV profiles, `authSource`, collections with 25-document field sampling, strict `find` / `aggregate` / `explain` request bodies, collection indexes, a JSON document view |
| Editor | Tabs per connection, SQL and Mongo JSON colouring, Ctrl+Enter runs the selection or the statement under the cursor (a read-only batch runs statement by statement), Ctrl+Shift+F formats, Ctrl+S saves the text to a file, per-engine starter queries, a Mongo request strip |
| Results | Virtualised rows, sort and filter on the fetched page, column visibility, cell copy, TSV / JSON copy and save, a row limit per tab |
| Editing | Double-click a cell to stage a new value, delete rows from the row menu, review the staged set in the Changes panel, Apply runs it in one transaction that rolls back unless every statement changes exactly one row. Needs a single-table result with a one-column primary key on a profile that is not read-only |
| Structure | Columns with PK / FK / not-null flags, foreign keys, indexes; create a table or add a column from a form over a fixed type list |
| Ask | Describe the query you want; the active tab's text goes along as context, result rows and credentials never do. Fenced `sql` / `mql` blocks in the answer insert into the editor with one click |
| Workspace | Up to 30 tabs and 50 history entries restored on start (`workspace.json`), window geometry in `ui.ron`; `REDROB_QUERY_NO_PREFS=1` keeps nothing |

SQL Server is not supported in this release. Writes typed into the editor are refused by the engine's
read-only gate; the only write paths are the Changes panel and the structure forms.

## Architecture

```text
apps/redrob-query  (eframe / egui 0.36 on wgpu)
  profiles · schema · editor · grid · changes · structure · Ask dock
                │  Workspace  (one method per former command, answered by Pending handles)
crates/redrob-query-core
  DataService  ──  SQLx (PostgreSQL / MySQL / SQLite)  ·  mongodb  ·  reqwest (console)
  query.rs read-only classification · cell_edits.rs · ddl.rs · keys.rs · profile_store.rs · secret.rs
  account.rs: Redrob Launcher's shared account.json
```

See [Architecture](docs/ARCHITECTURE.md), [Security](docs/SECURITY.md) and the
[DBeaver workflow map](docs/DBEAVER-PORTING-MAP.md).

## Install

Every release carries, under the names Redrob Launcher reads:

| Platform | Assets |
|---|---|
| Linux x86_64 | `redrob-query-<version>-linux-x86_64.tar.gz`, `redrob-query-<version>-linux-x86_64.deb` |
| macOS arm64 | `redrob-query-<version>-macos-arm64.dmg`, `redrob-query-<version>-macos-arm64.zip` (signed, notarized, stapled) |
| Windows x64 | `redrob-query-<version>-windows-x64-setup.exe` (English + Korean setup), `redrob-query-<version>-windows-x64-portable.zip` (signed) |

plus `SHA256SUMS.txt` and `redrob-launcher.json`. The Windows setup installs per user into
`%LOCALAPPDATA%\Programs\Redrob Query`; the installed product is **ProductName `Redrob Query`** with
the executable **`bin\redrob-query.exe`** and the Settings > Apps entry `RedrobQuery`
(publisher `Redrob`) -- the names Redrob Launcher detects it by. Details:
[installer/windows/README.md](installer/windows/README.md).

## Build from source

Rust comes from rustup; `rust-toolchain.toml` pins 1.95.0 with rustfmt and Clippy. On Linux the
shell links the X11 / Wayland / GL development headers and GTK 3 (for the file dialogs):

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libx11-dev libxrandr-dev libxi-dev \
  libgl1-mesa-dev libgtk-3-dev libdbus-1-dev
cargo run --release -p redrob-query
```

`redrob-query --version` prints the version and exits; it needs no display.

### Validation

```bash
cargo fmt --all -- --check
cargo clippy -p redrob-query -p redrob-query-core --all-targets --locked -- -D warnings
cargo test -p redrob-query-core --locked
python3 scripts/check-brand.py
```

The default test run is hermetic and reports three connector tests as ignored; they need live
servers. To run them, export disposable service URLs and opt in:

```bash
export REDROB_TEST_POSTGRES_URL='postgresql://…'
export REDROB_TEST_MYSQL_URL='mysql://…'
export REDROB_TEST_MONGO_URL='mongodb://…'
export REDROB_TEST_MONGO_DATABASE='redrob_test' # optional
cargo test -p redrob-query-core --locked -- --ignored
```

### Brand assets

Every file under `resources/icons`, `resources/brand` and the installer `res/` folders is built from
the design system's published files by `scripts/build-brand-assets.py --design-system <path>` and
pinned by sha256 in `brand/SOURCES.json`; `scripts/check-brand.py` (CI and the release) refuses a
hand-edited or unpinned file.

### Release

The **Release** workflow (`gh workflow run release.yml --ref main -f tag=vX.Y.Z`) builds the three
targets from the one cargo binary, signs Windows through Azure Artifact Signing with the job's OpenID
Connect login, signs and notarizes macOS with the organisation's Developer ID, and the shared
`redrob-labs/.github` publish job writes `SHA256SUMS.txt` and `redrob-launcher.json` and publishes the
GitHub Release as latest. The tag must be on `main`, equal the workspace version in `Cargo.toml`, and
have a `## <version>` section in [CHANGELOG.md](CHANGELOG.md). `-f dry_run=true` builds and signs a
branch and publishes nothing.

## Local data

| File | What |
|---|---|
| `connections.json` | Non-secret profile metadata (the engine's own file, atomic, journaled) |
| `account.json` | This app's own Redrob sign-in, if the launcher's Connect or you wrote one; wins over the shared file |
| `workspace.json` | Open tabs (query text only) and the run history; never result rows, passwords or prompts |
| `ui.ron` | eframe's window geometry and panel sizes |

Directory: `%APPDATA%\Redrob Query` on Windows, `~/Library/Application Support/Redrob Query` on macOS,
`$XDG_CONFIG_HOME/redrob-query` (`~/.config/redrob-query`) elsewhere; `REDROB_QUERY_CONFIG_DIR`
overrides it. Query text is plain text and may contain sensitive literals; set
`REDROB_QUERY_NO_PREFS=1` to keep no workspace or window state.

## Security summary

- Non-secret profile metadata is stored in plaintext `connections.json`; passwords use the OS keyring.
- A secret-free fsynced journal, staged keyring entry and exclusive profile lock make updates
  recoverable and fail closed under unsafe recovery or concurrent ownership.
- Typed queries are reads only: each run accepts read-only statements (several only when every one
  is read-only, each executed on its own), with PostgreSQL / MySQL read-only transactions and a
  fail-closed SQLite PRAGMA allowlist. Writes happen only through reviewed edits and the structure
  forms, on a profile that is not read-only, with every value bound.
- Mongo requests are strict and bounded; aggregate `$out` and `$merge` are rejected recursively.
- Enabled PostgreSQL / MySQL TLS verifies certificate identity with public trust roots.
- Ask sends the prompt and the active tab's text to the Redrob console with the account's bearer
  key. Result rows and credentials are not attached, but sensitive literals you typed into either
  text are transmitted.

Read the complete [security model and limitations](docs/SECURITY.md).

## The Redrob account

Ask uses the account Redrob Launcher signed this computer in with: `account.json` in
`$XDG_CONFIG_HOME/redrob-shared`, `~/Library/Application Support/ai.redrob.shared` or
`%APPDATA%\Redrob\Shared` (`REDROB_SHARED_DIR` overrides), read on every request and watched while
the app is open, as the launcher's `docs/shared-account.md` contract asks. `REDROB_API_KEY` in the
environment, then this app's own `account.json`, win over it. The key is sent only as a bearer token to
`{apiBaseUrl}/chat/completions` with model `auto`; the console's own error message is shown when it
refuses. Without an account the Ask dock says so and offers to open the launcher; this app has no
sign-in of its own.

## Current boundaries

- No free-form write SQL: changes go through the reviewed edit set and the table forms. Tables need a
  one-column primary key to be edited; MongoDB results and composite keys are not editable yet.
- Sorting and filtering apply to the fetched page (up to the tab's limit), not to the database.
- No custom CA or client-certificate UI, SSH tunnel, ER diagram, data-transfer pipeline, compare or
  migration tooling, driver marketplace, or SQL Server connector.
- Mongo sampled metadata is a bounded hint, not a schema.

## License and attribution

Redrob Query is licensed under the [GNU General Public License, version 3 or later](LICENSE). It was
Apache-2.0 until it began incorporating Beekeeper Studio Community Edition source, which is
GPL-3.0-or-later; the stronger copyleft travels with the code. See [NOTICE](NOTICE) for attribution
and the trademark reservation, and [UPSTREAM_NOTICES.md](UPSTREAM_NOTICES.md) for the upstream code
notices. Third-party components remain subject to their own licenses.
