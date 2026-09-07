# Architecture

Redrob Data 0.1 separates browser-demo behavior, renderer state, native IPC, and database/AI runtime responsibilities. Java and the JVM are not part of the product.

## Layers

### React renderer

`src/` contains the React 19 UI. Monaco provides SQL/MQL editing; TanStack Table/Virtual render typed result pages. Components do not access databases, files, keyrings, or Redrob directly.

`workspaceStore.ts` owns:

- explicit desktop profile selection and demo auto-selection;
- connection-owned query tabs and engine-specific starter queries;
- metadata, query, demo-mutation, and AI state;
- generation/request tokens that discard stale async completions after connection or tab changes;
- browser-demo edit review and transient notifications.

### DataBridge boundary

`DataBridge` defines the renderer contract.

- `DemoBridge` is deterministic browser memory. It provides engine-aware sample metadata/results, per-profile relational fixtures, read-only Mongo documents, and local AI responses.
- `TauriBridge` maps renderer values to a narrow Tauri command allowlist and converts typed Rust values back into grid rows. Desktop mutation calls reject locally.

### Tauri shell

`src-tauri` creates one `DataService` in the Tauri app-data directory and registers only profile lifecycle, connection lifecycle, metadata, read-query, AI-key, and narrow AI-assistant commands. Database mutation commands are intentionally absent in the read-only desktop preview.

The window uses an explicit CSP and no shell/filesystem plugin grants.

### Rust core

`crates/redrob-core` owns domain validation and trusted operations:

- concrete SQLx `PgPool`, `MySqlPool`, and `SqlitePool` connectors;
- the official MongoDB Rust driver;
- typed scalar conversion that preserves exact wide integers/decimals, binary, temporal values, UUID, JSON, and BSON;
- metadata loading and bounded result pagination;
- conservative relational and Mongo read policies;
- profile persistence, OS keyring access, crash recovery, and process ownership;
- native Redrob AI request construction and HTTPS transport.

## Connection lifecycle

Desktop startup lists stored profiles but does not connect or select one. Selecting a profile creates/activates an engine-owned starter tab and loads root metadata; the Rust service lazily opens the connection at that boundary. Switching profiles invalidates outstanding renderer generations and clears incompatible results, edits, and AI history.

Persistent profiles contain only non-secret connection metadata. SQLite skips keyring reads. Other connectors retrieve the secret only when connecting.

## Engine paths

### PostgreSQL and MySQL

Each engine has concrete metadata, query, parameter-binding, scalar-decoding, and transaction code. Read queries run in a server read-only transaction and are rolled back after streaming the bounded page.

### SQLite

File profiles use a concrete SQLite pool. The built-in `Demo SQLite` profile uses one in-memory connection and is seeded on first connect. User reads pass the single-statement classifier and a strict read-only PRAGMA allowlist.

### MongoDB

The renderer submits a JSON envelope for `find` or `aggregate`. The core validates operation-specific fields, sizes, limits, stages, write operators, and explain verbosity before driver execution. Metadata lists collections and samples one document for field hints.

## AI path

The renderer may send only prompt, optional active query, database kind, and explicit query language. Rust rejects unknown fields and builds the provider model/messages itself. The API key is sourced from `REDROB_API_KEY` or the OS keyring and is used only as the HTTPS bearer credential.

## Deliberate preview boundaries

- Desktop results do not include edit-source identity and mutation IPC is not registered.
- Connectors are compiled in; there is no downloadable driver/plugin system.
- Query tabs and history are renderer memory, not durable projects.
- Mongo field metadata is sampled rather than inferred across a collection.
- Platform packaging is host-native and currently has no signing, updater, or release automation.
