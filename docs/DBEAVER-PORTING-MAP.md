# DBeaver workflow map

Redrob Data is a clean Rust/React implementation informed by DBeaver Community workflows. It is not a source port, fork, plugin host, or compatibility layer, and it contains no DBeaver code or branding.

| DBeaver Community workflow | Redrob Data 0.1 equivalent | Current boundary |
|---|---|---|
| Database Navigator | Profile picker plus lazy searchable database/schema/table/view/column tree; activity-rail search focuses the filter | Lightweight metadata; Mongo fields are merged from at most 25 sampled documents |
| New Connection wizard | Compiled-in profile modal with create/test/edit, connect/disconnect/remove, TLS, Mongo SRV, and `authSource` | PostgreSQL/MySQL/SQLite/MongoDB only; no driver manager, SQL Server, SSH, private CA, or cloud-auth plugin |
| SQL Editor | Monaco connection-owned SQL/MQL tabs, engine starters, execution, copy, formatting, shortcuts, and bounded local desktop restoration/history | No multi-statement script runner, plan UI, durable project files, or transaction toolbar |
| Data Viewer | Typed virtualized rows, bridge/server-bounded pages, 25/50/100/250 sizes, current-page sort/filter, column visibility, messages, metrics, and visible CSV export | Desktop is read-only; sort/filter do not span unloaded pages |
| Data Editor | Browser-demo relational cell staging, review, exact-previous-value checks, and atomic fixture apply | Sample memory only; no desktop/native editing |
| Object properties | Expandable tables/views/columns and bounded Mongo sampled-field hints | No rich DDL, constraint/index/dependency panels, authoritative Mongo schema, or ER diagrams |
| Connection security | OS-keyring secrets, verified TLS modes, strict non-secret profiles, journaled updates, and exclusive store ownership | Public trust roots only; no custom CA/client-certificate UI |
| Local workspace | Up to 30 query tabs (including empty drafts) and 50 nonempty successful first-page history entries in versioned desktop local storage | Plaintext query-only convenience state, user-clearable; not encrypted projects; browser demo is in-memory |
| Tasks/data transfer | No equivalent | No scheduler, import/export pipeline beyond visible CSV, compare, sync, or migration tooling |
| Extension ecosystem | No equivalent | Connectors are compiled Rust implementations, not Eclipse/OSGi/JDBC plugins |
| AI assistance | Redrob-specific engine-aware SQL/MQL assistant | Sends prompt and optional active query to Redrob; not a DBeaver-derived feature |

## Architectural replacement

| DBeaver-era concept | Redrob Data choice |
|---|---|
| Java/JVM | Rust 1.94 native core |
| Eclipse RCP/SWT | Tauri 2 system webview + React 19 |
| JDBC drivers | Concrete SQLx PostgreSQL/MySQL/SQLite pools and official MongoDB Rust driver |
| Eclipse jobs/state | Tokio async runtime plus Zustand request generations |
| Secure storage abstraction | OS keyring with secret-free journaled profile coordination |
| Plugin command surface | Explicit Tauri IPC allowlist and minimal capability grant |

## Scope statement

Version 0.1 is a focused modern read-only desktop database workspace, not full DBeaver parity. Shipped scope includes profile lifecycle, metadata browsing, guarded bounded queries, typed/paged results, current-page controls, CSV export, query-only local desktop restoration/history, deterministic browser sample workflows, and Redrob AI context.

Native editing, driver plugins, SQL Server, custom certificate authorities, SSH/cloud authentication, data transfer, compare/migrate, ER modeling, task scheduling, administration/monitoring, signing, and updating are not implemented.
