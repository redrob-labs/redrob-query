# Architecture

Redrob Data 0.1 separates browser-demo behavior, renderer state, native IPC, and database/AI runtime responsibilities. Java and the JVM are not part of the product.

## Layers

### React renderer

`src/` contains the React 19 UI. Monaco provides SQL/MQL editing and TanStack Virtual renders typed result pages. Components do not directly access databases, files, keyrings, or Redrob.

`workspaceStore.ts` owns:

- explicit desktop profile selection/connect/disconnect and demo auto-selection;
- connection-owned query tabs and engine-specific starter queries;
- metadata, query, paging, demo-mutation, and AI state;
- generation/request tokens that discard stale async completions after connection, tab, query-text, or mutation changes;
- 25/50/100/250 page sizes and bridge-provided `offset`/`nextOffset` continuation;
- browser-demo atomic edit review and transient notifications;
- desktop-only query tab/history restoration without automatic selection, connection, or execution.

`workspacePersistence.ts` stores a versioned, bounded query-only snapshot in renderer local storage: at most 30 tabs, 50 history entries, 200,000 characters per query, and a conservative aggregate budget. Empty tab drafts are valid; history accepts only nonempty queries from successful first-page executions. It validates profile ownership, engine/language compatibility, unique IDs, and schema version. Corrupt or unsafe state is removed with a visible warning. Rows, credentials, and AI prompts are never included. Users can disable and clear local saving.

### DataBridge boundary

`DataBridge` defines the renderer contract.

- `DemoBridge` is deterministic browser memory. It provides engine-aware sample metadata/results, connection lifecycle simulation, bounded pages, isolated per-profile relational fixtures, read-only Mongo documents, and local AI responses. Mutation batches are fully prevalidated against exact previous values before any fixture is changed.
- `TauriBridge` maps renderer values to a narrow Tauri command allowlist, including profile warnings, profile removal, connect, and disconnect. It converts typed Rust values into grid rows and rejects desktop mutation calls locally. Profile saves return `{ profile, warning? }` and removals return `{ warning? }`. A warning means the durable mutation committed but bounded cleanup remains, so the renderer first reconciles the returned profile/removal and then displays the warning; a rejected promise still means the mutation did not commit.

### Tauri shell

`src-tauri` creates one `DataService` in the Tauri app-data directory. It registers profile lifecycle, connection lifecycle, metadata, read-query, AI-key, and narrow AI-assistant commands. Database mutation commands are intentionally absent.

The single window is explicitly labeled `main`, uses an explicit CSP, and grants only `core:default`; there are no shell or filesystem plugin permissions. Redrob HTTPS is performed by Rust rather than by the renderer, so the renderer CSP does not grant a Redrob network origin.

### Rust core

`crates/redrob-core` owns domain validation and trusted operations:

- concrete SQLx `PgPool`, `MySqlPool`, and `SqlitePool` connectors;
- the official MongoDB Rust driver;
- typed scalar conversion preserving wide integers/decimals, binary, temporal values, UUID, JSON, and BSON;
- metadata loading and bounded result pagination;
- conservative relational and Mongo read policies;
- profile persistence, OS keyring access, crash recovery, and process ownership;
- native Redrob AI request construction and bounded HTTPS transport.

SQL Server exists only as a rejected compatibility enum value. Profile normalization and AI request construction both fail it explicitly; the renderer does not offer it.

## Connection and profile lifecycle

Desktop startup lists stored profiles and independently retrieves sanitized recovery/load warnings, but it does not select, connect, load metadata, or execute a query. A warning-diagnostics failure does not discard a successfully loaded connection list.

The navigator supports edit, connect/disconnect, and removal for user profiles. Built-in profiles cannot be edited or removed. Editing with a blank password preserves the existing keyring secret only when the engine, host/port endpoint, database, username, TLS policy, SRV mode, and Mongo authentication scope are unchanged; otherwise a replacement password is mandatory. For an unchanged-scope connection test, trusted Rust may use that saved keyring credential without returning it to the renderer; changed-scope tests require explicit re-entry. Database type is immutable for an existing profile, so changing engines requires a new profile.

Selecting a disconnected profile prepares its engine-owned tab but does not connect. An explicit connect command updates lifecycle state, activates the profile, and then loads root metadata. Switching profiles invalidates outstanding renderer generations and clears incompatible results, staged edits, paging, and AI history. Disconnect closes the native connection and clears live metadata, results, and staged edits while retaining the selected profile's tabs and query history. The renderer serializes save, connect, disconnect, and remove invocations through one lifecycle queue, matching the native lifecycle mutex so completion order cannot republish a deleted profile or discard a committed removal. A committed save closes any active native connection for that profile and returns its reconciled disconnected profile. Confirmed profile removal deletes its owned local tabs/history and stored credential. Post-commit cleanup warnings do not roll back either renderer transition; they are shown only after state reconciliation.

Persistent profiles contain only non-secret connection metadata. SQLite skips keyring reads. Other connectors retrieve the secret only when connecting.

## Engine paths

### PostgreSQL and MySQL

Each engine has concrete metadata, query, parameter-binding, scalar-decoding, and transaction code. Read queries execute in a server read-only transaction and are rolled back after a bounded page is streamed.

### SQLite

User file profiles are opened with `mode=ro`. The writable built-in/test path uses SQLite's in-memory connection form (`:memory:` mapped to `sqlite::memory:`). The built-in `Demo SQLite` profile uses one in-memory connection and is seeded on first connect. Queries pass the single-statement classifier and strict read-only PRAGMA allowlist.

### MongoDB

The renderer submits a JSON envelope for `find` or `aggregate`. The core validates operation-specific fields, sizes, limits, stages, write operators, and explain verbosity before driver execution. Metadata samples at most 25 documents and deterministically merges top-level BSON type unions, presence counts, and nullability hints. This is bounded metadata assistance, not complete schema inference.

## Result path

A query result carries typed columns/rows, duration, message, page offset/limit, optional continuation, and truncation state. Paging reruns the same active tab query with a bounded offset. Sorting, filtering, hidden columns, messages, and CSV export are renderer views of the current page. View controls reset when a new query/page result arrives, while accepted demo edits can update the current result without erasing the view.

Desktop results contain no editable source identity. Browser demo relational fixtures can expose a constrained edit source solely for sample-data review.

## AI path

The renderer may send only prompt, optional active query, database kind, and explicit query language. Rust rejects unknown fields and constructs the provider model, system instruction, messages, endpoint, and authorization itself. The API key comes from nonempty `REDROB_API_KEY` or the OS keyring and is used only as the HTTPS bearer credential.

## Deliberate boundaries

- No desktop mutation IPC or trusted native edit-source identity.
- Connectors are compiled in; there is no downloadable driver/plugin system.
- Desktop query tabs/history are local plaintext convenience state, not encrypted projects; browser state is in-memory only.
- Mongo field metadata is bounded sampling rather than authoritative inference.
- Sorting/filtering are current-page operations even though paging is bridge/server bounded.
- No custom CA, SSH, cloud-auth, data transfer, ER/admin suite, signing, updater, or release automation.
