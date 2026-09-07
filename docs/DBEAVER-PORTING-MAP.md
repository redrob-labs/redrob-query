# DBeaver workflow map

Redrob Data is a clean Rust/React implementation informed by DBeaver Community workflows. It is not a source port, fork, plugin host, or compatibility layer, and it includes no DBeaver code or branding.

| DBeaver Community workflow | Redrob Data 0.1 equivalent | Current boundary |
|---|---|---|
| Database Navigator | Connection picker and lazy searchable database/schema/table/view/column tree | Lightweight metadata only; Mongo fields sample one document |
| New Connection wizard | Compiled-in connection modal with defaults, test/save, TLS, Mongo SRV and `authSource` | No downloadable driver manager, SSH tunnel, private CA, or cloud-auth plugin |
| SQL Editor | Monaco connection-owned tabs, SQL/MQL starters, execute/copy/basic formatting, shortcuts | No durable scripts/history, multi-statement runner, SQL plan UI, or transaction toolbar |
| Data Viewer | Typed virtualized rows, local filtering, row metrics, CSV export | Desktop is read-only; no server paging/sorting/column manager |
| Data Editor | Browser-demo relational cell staging and review | Sample-memory only; no desktop/native editing in 0.1 |
| Object properties | Expandable tables/views/columns and Mongo sampled fields | No rich DDL, constraints/index panels, dependencies, or ER diagrams |
| Connection security | OS-keyring credentials, verified TLS modes, strict non-secret profiles | Public trust roots only; no custom CA/client certificate UI |
| Tasks/data transfer | No equivalent yet | No scheduler, import/export pipeline, compare, sync, or migration tooling |
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
| Plugin command surface | Explicit Tauri IPC allowlist |

## Parity statement

The 0.1 goal is a focused modern database workspace, not full DBeaver parity. Shipped scope is connection management, metadata browsing, guarded read queries, typed results, CSV export, a deterministic browser demo, and Redrob AI context. Administrative tooling, native editing, driver plugins, data transfer, compare/migrate, ER modeling, task scheduling, monitoring, and SQL Server are future work.
