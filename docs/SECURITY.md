# Security model

This document describes Redrob Data 0.1 behavior; it is not a guarantee that arbitrary database functions are side-effect free.

## Trust boundaries

The Tauri renderer is untrusted for direct database, profile-file, keyring, and Redrob access. It receives an explicit IPC allowlist. Desktop database mutation commands are not registered, saved desktop profiles are read-only, and native result rows expose no editable source identity.

The browser demo has no native boundary. It operates only on deterministic in-memory fixtures and does not contact databases or Redrob. Demo relational mutations first validate the entire batch, source identity, primary-key restrictions, and exact previous values; any stale or invalid item rejects the full batch before a write.

SQL Server is unsupported. It is absent from the connection UI and explicitly rejected by Rust profile and AI validation rather than falling through to another connector.

## Profiles and credentials

`connections.json` is plaintext non-secret metadata in the Tauri app-data directory. It may contain host, port, database, username, SQLite path, TLS/SRV choices, and validated Mongo `authSource`. Passwords and the Redrob API key are stored separately in the OS keyring. They are never serialized into the profile file or transaction journal.

Persistent desktop mode fails closed when the keyring or profile store is unavailable. A process-memory secret fallback exists only for ephemeral/test services. Saved-profile warnings are sanitized before entering renderer state. Failure of the optional warning query does not hide an otherwise valid profile list. Trusted Rust may resolve an existing keyring credential for connect or a connection test only when engine, host/port, database, username, TLS policy, SRV mode, and Mongo `authSource` are unchanged; the secret never enters renderer state, and any changed scope requires explicit password re-entry.

### Crash consistency and ownership

A profile update uses a secret-free journal and random keyring staging UUID:

1. atomically write and fsync the target-profile journal;
2. stage the new secret in the keyring;
3. atomically replace and fsync `connections.json`;
4. promote the staged secret to the profile UUID;
5. mark the journal applied, remove staging, and remove the journal.

Startup reconciles an interrupted operation before publishing persisted profiles. Delete operations are journaled and completed forward. A failure before durable target visibility remains an error and preserves native and renderer state. If the target is already durable but staging-keyring, credential, or journal cleanup fails, IPC returns committed success with a fixed, bounded, non-secret warning; renderer state is reconciled immediately and restart retries cleanup. Journal/profile reads are bounded and strictly validated. A recovery state that cannot be proven safe exposes only the built-in demo and blocks persistent operations.

An empty adjacent lock file is held with an OS-level exclusive lock for the persistent service lifetime. Another instance cannot reconcile, read, or write the same profile store and receives a sanitized fail-closed warning. Journal replacement/removal also verifies the operation UUID.

## Local query workspace

Desktop query convenience state is stored in renderer local storage under versioned key `redrob-data.workspace.v1`. It is plaintext and may contain sensitive literals embedded in query text. The snapshot contains only bounded tab/history metadata and query text—never result rows, profile credentials, passwords, or AI prompts.

Restoration validates schema version, profile ownership, SQL/MQL compatibility with the current engine, unique IDs, per-query length, entry counts, and an aggregate storage budget. An empty tab draft is valid and survives restart; history entries must contain a nonempty query from a successful first-page execution. Corrupt, oversized, duplicate, or incompatible entries are rejected and surfaced through a generic startup warning. Storage failure changes the UI to **Local save unavailable** instead of reporting a false success. Users can stop saving and clear the snapshot; the opt-out is stored separately and survives restart until explicitly re-enabled. Browser-demo workspace state is never persisted.

Restored state does not auto-select a profile, connect, execute a query, or send an AI request. An existing profile's database type is immutable; changes to the host/port endpoint, database, username, TLS policy, SRV mode, or Mongo authentication source require an explicit replacement password, preventing a stored credential from being silently reused in a different trust scope.

## Query policy

Relational desktop execution accepts exactly one nonempty statement classified as read-only.

- batches, writes, DDL, transaction control, attachment, and unknown statements are rejected;
- MySQL `INTO OUTFILE`/`INTO DUMPFILE`, including executable-comment forms, are rejected;
- SQLite uses an explicit query-form PRAGMA allowlist and rejects maintenance, setter, and unknown PRAGMAs;
- user SQLite file profiles open with `mode=ro`; the writable built-in/test path is restricted to SQLite's in-memory connection form (`:memory:` / `sqlite::memory:`);
- PostgreSQL and MySQL execute inside server read-only transactions and roll back;
- query length, limit, offset, timeout, and returned pages are bounded.

This combines conservative lexical routing with database controls. SQLite does not provide the same server read-only transaction boundary, and custom server functions can have engine-specific behavior. Use least-privilege read-only accounts.

MongoDB accepts strict bounded `find` and `aggregate` envelopes. Query JSON is capped at 1 MiB; pipelines are capped at 100 stages and 64 KiB per stage; unknown/incompatible fields are rejected; `$out` and `$merge` are rejected recursively; limits and timeouts are bounded. Desktop exposes no Mongo mutation command. Metadata reads sample at most 25 documents and merge top-level field type/presence/nullability hints; they do not assert a complete schema.

## Transport security

When TLS is enabled:

- PostgreSQL uses `sslmode=verify-full`;
- MySQL uses `ssl-mode=VERIFY_IDENTITY`;
- MongoDB uses `tls=true`.

PostgreSQL/MySQL certificate and hostname verification use public trust roots. Custom/private/self-signed CA configuration, client certificates, SSH tunnels, and cloud-auth plugins are not supported. TLS can be disabled explicitly for compatible trusted local environments.

## Redrob AI egress

Desktop AI calls `https://console.redrob.ai/api/backend/v1/chat/completions` with model `auto`. The renderer CSP does not grant this origin; the trusted Rust HTTPS client performs the request.

The renderer-facing request permits only:

- user prompt, maximum 8 KiB;
- optional active query, maximum 64 KiB;
- supported database kind;
- explicit compatible query language.

Rust constructs the system instruction, model, temperature, endpoint, and message shape. Unknown fields—including renderer-provided message arrays, result rows, credentials, model, or endpoint—are rejected or omitted. SQL Server context is rejected. Responses are capped at 2 MiB. The API key is used as the HTTPS bearer credential and is not inserted into messages.

Database result rows and credentials are not automatically attached. Anything manually typed or pasted into the prompt or active query is sent. A nonempty `REDROB_API_KEY` environment value takes precedence over the keyring value.

## Error handling

Database and connection errors are sanitized and bounded before crossing IPC. URLs and embedded credentials are redacted. Debug formatting for connection and AI request models omits identity and content. Startup warning text removes control characters and is length-bounded.

## Recommended deployment posture

- Use least-privilege, read-only database accounts.
- Keep TLS verification enabled and use certificates chained to public trust roots.
- Do not use production credentials in browser demos or live integration tests.
- Treat profile metadata, local query text/history, and exported CSV files as sensitive.
- Disable and clear local query saving when plaintext query retention is inappropriate.
- Review prompt and active-query text before sending it to Redrob.
