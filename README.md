# Redrob Data

![Redrob mark](src/assets/redrob-mark-512.png)

Redrob Data is a JVM-free, AI-assisted database workspace built with Rust 1.94, Tauri 2, React 19, and TypeScript. Version 0.1 provides a guarded read-only desktop workspace for PostgreSQL, MySQL, SQLite, and MongoDB, plus an interactive browser demo backed only by deterministic in-memory sample data.

The workflow is informed by DBeaver Community, but Redrob Data is a clean implementation. It includes no DBeaver source code or branding and no Eclipse RCP, OSGi, JDBC, Java, or JVM runtime.

## Shipped scope

| Area | Capability |
|---|---|
| Profiles | Create, test, edit, connect, disconnect, and remove user profiles; built-in profiles are immutable |
| PostgreSQL | Identity-verifying TLS, metadata, bounded read-only SQL, native scalar decoding |
| MySQL | Identity-verifying TLS, metadata, bounded read-only SQL, unsigned/decimal/temporal/BIT decoding |
| SQLite | Read-only file profiles using `mode=ro`, metadata, and guarded single-statement reads; native demo uses `:memory:` |
| MongoDB | Standard/SRV profiles, separate `authSource`, collections, bounded 25-document field sampling, strict `find`/`aggregate`/explain JSON |
| Results | 25/50/100/250-row paging, typed virtualized rows, current-page sort/filter, column visibility, execution messages, and visible CSV export |
| Query workspace | Connection-owned SQL/MQL tabs, engine starters, local desktop restoration, bounded execution history, formatting, and shortcuts |
| Redrob AI | Engine-aware SQL/MQL generation and explanation through model `auto` |
| Browser demo | Sample PostgreSQL/MySQL/SQLite/Mongo workflows, local AI responses, and atomic in-memory relational edit review |

SQL Server is not supported in this release: it is absent from the connection UI and rejected by both profile and AI validation paths.

Desktop query results are intentionally read-only. Mutation commands are not registered with Tauri, and persistent desktop profiles are read-only. Browser-demo edits validate the entire batch, including previous values, before atomically changing a per-profile in-memory fixture; reload discards them.

## Architecture

```text
React 19 + Monaco + TanStack Virtual
                │
         Zustand workspace state
                │ DataBridge
          ┌─────┴───────────┐
     DemoBridge         Tauri IPC allowlist
   (browser memory)              │
                            redrob-core (Rust)
                       ┌─────────┼──────────┐
                 SQLx concrete  MongoDB   Redrob AI
                 PG/MySQL/SQLite driver   HTTPS client
```

See [Architecture](docs/ARCHITECTURE.md), [Security](docs/SECURITY.md), [Demo guide](docs/DEMO.md), and the [DBeaver workflow map](docs/DBEAVER-PORTING-MAP.md).

## Quick start: browser demo

The browser demo does not connect to a database or Redrob service and does not persist its workspace.

```bash
npm ci
npm run dev
```

Open `http://127.0.0.1:1420`. The demo starts with the connected **Acme Warehouse** sample profile. It supports deterministic metadata and queries, two result pages at the default 50-row size, local AI responses, CSV export, profile lifecycle simulation, and reviewed in-memory relational edits.

Requirements: Node.js 22.12+ and npm 11+.

## Desktop development

1. Install Node.js 22.12+, npm 11+, and the platform packages from the [Tauri v2 prerequisites guide](https://v2.tauri.app/start/prerequisites/).
2. Install Rust through rustup; `rust-toolchain.toml` pins Rust 1.94.0, rustfmt, and Clippy.
3. Install dependencies and launch Tauri:

```bash
npm ci
npm run tauri dev
```

Build a package for the current host platform with:

```bash
npm run tauri build
```

Linux requires the WebKitGTK 4.1, JavaScriptCoreGTK 4.1, libsoup 3, and librsvg development metadata expected by Tauri. Native build/package validation in the current Amazon Linux sandbox is blocked because those host packages are unavailable; frontend production builds and all `redrob-core` checks remain runnable.

The repository contains standard Tauri PNG, ICNS, ICO, iOS, and Android icons under `src-tauri/icons/`. It does not provide signing, notarization, an updater, or cross-platform release automation.

## Desktop local workspace data

Desktop mode stores up to 30 open query tabs and 50 successful first-page history entries in versioned renderer local storage. Only query text and tab/history metadata are stored, never result rows, database credentials, or AI prompts. Empty tab drafts are valid and restored; execution history retains only nonempty queries that completed successfully on the first page. Query text is plaintext and may itself contain sensitive literals, so the history menu provides **Stop saving & clear local data**. That opt-out is stored separately and remains disabled across restarts until the user explicitly re-enables saving. Invalid, duplicate, engine-incompatible, oversized, or corrupt entries are rejected; storage failure is shown instead of claiming a successful save.

Browser-demo tabs and history remain in memory for the current page session only. Selecting, restoring, or loading desktop state never auto-connects or auto-runs a query.

## Validation

```bash
npm run release:check
npm run typecheck
npm test
npm run test:coverage
npm run build
npm audit --audit-level=low
cargo fmt --all -- --check
cargo clippy --locked -p redrob-core --all-targets --all-features -- -D warnings
cargo test --locked -p redrob-core --all-features
cargo audit --file Cargo.lock
```

`npm run release:check` verifies npm, Cargo, Tauri, lockfile, Rust toolchain, product name, bundle identifier, window label, and development URL metadata. Coverage has global regression floors of 85% statements/lines, 75% branches, and 65% functions.

The default Rust core run executes hermetic tests and reports three external connector tests as ignored. It is not live-server evidence. To run the ignored gates, export disposable service URLs and explicitly opt in:

```bash
export REDROB_TEST_POSTGRES_URL='postgresql://…'
export REDROB_TEST_MYSQL_URL='mysql://…'
export REDROB_TEST_MONGO_URL='mongodb://…'
export REDROB_TEST_MONGO_DATABASE='redrob_test' # optional
cargo test --locked -p redrob-core --all-features -- --ignored
```

An explicitly run live test fails when its required URL is absent. Use non-production credentials; the Mongo gate creates and drops only a UUID-named collection. Rust does not automatically load `.env` files.

`npm run check` also includes workspace-wide Rust checks and therefore requires native Tauri system packages on Linux.

## Security summary

- Non-secret profile metadata is stored in plaintext `connections.json`; passwords and the Redrob key use the OS keyring.
- A secret-free fsynced journal, staged keyring entry, and exclusive profile lock make updates recoverable and fail closed under unsafe recovery or concurrent ownership.
- Desktop relational reads accept one conservatively classified read-only statement. PostgreSQL/MySQL add read-only transactions; SQLite file profiles open with `mode=ro` and use a fail-closed PRAGMA allowlist.
- Mongo requests are strict and bounded; aggregate `$out` and `$merge` are rejected recursively. Metadata merges top-level type/presence/nullability hints from at most 25 documents and is not a complete schema.
- Enabled PostgreSQL/MySQL TLS verifies certificate identity with public trust roots. Private/self-signed CA configuration is unavailable.
- Desktop AI sends the prompt and optional active query to Redrob. Result rows and credentials are not attached, but sensitive literals manually included in either text are transmitted.

Read the complete [security model and limitations](docs/SECURITY.md).

## Current boundaries

- No desktop/native result mutation; browser-demo editing is sample-only and in-memory.
- Paging is server/bridge bounded, while sorting and filtering apply only to the currently loaded page.
- No custom CA/client-certificate UI, SSH tunnel, cloud-auth plugin, script runner, ER diagram, administration suite, data-transfer pipeline, compare/migration tooling, driver marketplace, SQL Server connector, signing, updater, or release automation.
- Mongo sampled metadata is a bounded hint, not authoritative collection schema inference.
- External PostgreSQL/MySQL/Mongo and Redrob API integration require user-supplied services/credentials and are not exercised by the browser demo.

## Redrob AI configuration

Use **Redrob settings** in the desktop app to store a key in the OS keyring. For development, a nonempty `REDROB_API_KEY` takes precedence over the saved key. The native client calls:

```text
https://console.redrob.ai/api/backend/v1/chat/completions
```

The provider request uses model `auto`. Browser-demo AI is local and deterministic; an entered demo key is not retained or sent.

## License and attribution

Redrob Data is licensed under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for DBeaver Community workflow attribution. Third-party components remain subject to their own licenses.
