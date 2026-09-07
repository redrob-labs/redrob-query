# Redrob Data

![Redrob Data mark](src/assets/redrob-data-mark.svg)

Redrob Data is a Java-free, AI-assisted database workspace built with Rust, Tauri 2, React, and TypeScript. Version 0.1 is a **read-only desktop query preview** for PostgreSQL, MySQL, SQLite, and MongoDB, plus an interactive browser demo with deterministic sample data and in-memory staged edits.

The workflow is informed by DBeaver Community, but Redrob Data is a clean implementation: it does not include DBeaver source code, branding, Eclipse RCP, OSGi, JDBC, Java, or a JVM.

## What works

| Area | Preview capability |
|---|---|
| PostgreSQL | Connection profiles, identity-verifying TLS, metadata, read-only SQL, native scalar decoding |
| MySQL | Connection profiles, identity-verifying TLS, metadata, read-only SQL, unsigned/decimal/temporal/BIT decoding |
| SQLite | File and built-in in-memory demo connections, metadata, guarded single-statement reads |
| MongoDB | Standard/SRV profiles, separate `authSource`, collections, sampled fields, bounded `find`/`aggregate`/explain JSON |
| Redrob AI | Engine-aware SQL/MQL generation and explanation via model `auto` |
| Browser demo | Sample-only PostgreSQL/SQLite/Mongo workflows, local AI responses, CSV export, in-memory edit review |
| SQL Server | Planned; visibly unavailable and never connected in this preview |

Desktop query results are intentionally read-only. Mutation commands are not registered with Tauri, and desktop profiles are persisted as read-only. Browser-demo edits affect only per-connection in-memory fixtures and disappear on reload.

## Architecture

```text
React 19 + Monaco + TanStack UI
             │
      Zustand workspace state
             │ DataBridge
       ┌─────┴───────────┐
  DemoBridge         Tauri invoke allowlist
  (browser memory)            │
                         redrob-core (Rust)
                    ┌─────────┼──────────┐
              SQLx concrete  MongoDB   Redrob AI
              PG/MySQL/SQLite driver   HTTPS client
```

See [Architecture](docs/ARCHITECTURE.md), [Security](docs/SECURITY.md), [Demo guide](docs/DEMO.md), and the [DBeaver workflow map](docs/DBEAVER-PORTING-MAP.md).

## Quick start: browser demo

The browser demo does not connect to a database or Redrob service.

```bash
npm ci
npm run dev
```

Open the printed local Vite URL. The demo starts with an Acme Warehouse sample profile and supports deterministic metadata, queries, local AI responses, CSV export, and reviewed in-memory relational edits.

Requirements: Node.js `^20.19.0` or `>=22.12.0` (Node 22+ recommended).

## Desktop development

1. Install Node.js and the platform prerequisites from the [Tauri v2 prerequisites guide](https://v2.tauri.app/start/prerequisites/).
2. Install Rust 1.94.0; `rust-toolchain.toml` pins Rust, rustfmt, and Clippy.
3. Install dependencies and launch Tauri:

```bash
npm ci
npm run tauri dev
```

Build a package for the current host platform with:

```bash
npm run tauri build
```

Linux requires the WebKitGTK/JavaScriptCoreGTK/libsoup development packages expected by Tauri. Packaging in the current Amazon Linux validation environment is blocked because `libsoup-3.0`, `javascriptcoregtk-4.1`, and `webkit2gtk-4.1` development metadata are unavailable; frontend and `redrob-core` builds do not require those host packages.

The repository generates standard Tauri PNG, ICNS, ICO, iOS, and Android icons under `src-tauri/icons/`. It does not yet provide signing, notarization, an updater, or cross-platform release automation.

## Validation

```bash
npm run typecheck
npm test
npm run build
npm audit --audit-level=low
cargo fmt --all -- --check
cargo clippy -p redrob-core --all-targets --all-features -- -D warnings
cargo test -p redrob-core --all-features
```

`npm run check` also runs workspace-wide Rust checks, which require the native Tauri system packages.

Optional live connector gates run only when their environment variables are exported. A default green Rust run does not imply that an external server was contacted.

```bash
export REDROB_TEST_POSTGRES_URL='postgresql://…'
export REDROB_TEST_MYSQL_URL='mysql://…'
export REDROB_TEST_MONGO_URL='mongodb://…'
export REDROB_TEST_MONGO_DATABASE='redrob_test' # optional
cargo test -p redrob-core --all-features live_
```

Use disposable databases and non-production credentials. The Mongo test creates and drops only a UUID-named collection. Rust does not automatically load `.env` files.

## Security summary

- Non-secret profile metadata is stored in plaintext `connections.json`; passwords and the Redrob key use the OS keyring.
- A secret-free, fsynced journal and staged keyring entry make profile/credential updates recoverable after interruption.
- An exclusive OS file lock prevents multiple processes from writing the same profile store; contenders fail closed.
- Desktop relational reads accept one conservatively classified read-only statement. PostgreSQL/MySQL add read-only transactions; SQLite uses a fail-closed PRAGMA allowlist.
- Mongo read requests are strict and bounded; aggregate `$out` and `$merge` are rejected recursively.
- Enabled PostgreSQL/MySQL TLS verifies certificate identity using public trust roots. Private/self-signed CA configuration is not available in 0.1.
- Desktop AI sends the prompt and optional active query to Redrob. Result rows and database credentials are not automatically attached, but anything manually placed in the prompt or query is transmitted.

Read the full [security model and limitations](docs/SECURITY.md).

## Preview limitations

- Desktop results are read-only; browser-demo editing is sample-data-only.
- Mongo field metadata samples one document and is not a complete schema.
- Query tabs/history are not persisted, and there is no script runner, ER diagram, administration suite, migration/compare tooling, driver plugin marketplace, custom CA support, SSH tunneling, or signed installer pipeline.
- Live PostgreSQL/MySQL/Mongo gates are available but require explicitly configured disposable services.

## Redrob AI configuration

Use **Redrob settings** in the desktop app to store a key in the OS keyring. For development, `REDROB_API_KEY` may be exported and takes precedence over the saved key. The native client calls:

```text
https://console.redrob.ai/api/backend/v1/chat/completions
```

The provider request uses model `auto`. Browser-demo AI is local and deterministic; it does not use or store a key.

## License and attribution

Redrob Data is licensed under [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for DBeaver Community workflow attribution. Third-party components remain subject to their own licenses.
