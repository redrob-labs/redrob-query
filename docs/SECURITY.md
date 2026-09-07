# Security model

This document describes Redrob Data 0.1 behavior, not a guarantee that arbitrary database functions are side-effect free.

## Trust boundaries

The Tauri renderer is untrusted for direct database and keyring access. It receives an explicit IPC allowlist. Desktop database mutation commands are not registered, saved desktop profiles are read-only, and native result rows do not expose editable source identity.

The browser demo has no native boundary: it operates only on deterministic in-memory fixtures and does not contact databases or Redrob.

## Profiles and credentials

`connections.json` is plaintext non-secret metadata in the Tauri app-data directory. It may contain host, port, database, username, SQLite path, TLS/SRV choices, and validated Mongo `authSource`. Passwords and the Redrob API key are stored separately in the OS keyring. They are never serialized into the profile file or transaction journal.

Persistent desktop mode fails closed when the keyring or profile store is unavailable. The process-memory secret fallback exists only for ephemeral/test services.

### Crash consistency

A profile update uses a secret-free journal and a random keyring staging UUID:

1. atomically write and fsync the target profile journal;
2. stage the new secret in the keyring;
3. atomically replace and fsync `connections.json`;
4. promote the staged secret to the profile UUID;
5. mark the journal applied, delete staging, and remove the journal.

Startup reconciles an interrupted operation before publishing persisted profiles. Delete operations are journaled and completed forward. Journal/profile reads are bounded and strictly validated. A recovery state that cannot be proven safe exposes only the built-in demo and blocks persistent operations.

An empty adjacent lock file is held with an OS-level exclusive lock for the persistent service lifetime. Another instance cannot reconcile, read, or write the same profile store and receives a sanitized fail-closed warning. Journal replacement/removal also verifies the operation UUID.

## Query policy

Relational desktop execution accepts exactly one nonempty statement classified as read-only.

- batches, writes, DDL, transaction control, attachment, and unknown statements are rejected;
- MySQL `INTO OUTFILE`/`INTO DUMPFILE`, including executable-comment forms, are rejected;
- SQLite uses an explicit query-form PRAGMA allowlist and rejects maintenance/setter/unknown PRAGMAs;
- PostgreSQL and MySQL execute inside server read-only transactions and roll back;
- query length/limit/offset/timeout values are bounded.

This combines conservative lexical routing with database controls. SQLite does not provide the same server read-only transaction boundary, and custom server functions may have engine-specific behavior. Use least-privilege database accounts.

MongoDB accepts strict bounded `find` and `aggregate` envelopes. Query JSON is capped at 1 MiB; pipelines are capped at 100 stages and 64 KiB per stage; unknown/incompatible fields are rejected; `$out` and `$merge` are rejected recursively; limits and timeouts are bounded. Desktop exposes no Mongo mutation command.

## Transport security

When TLS is enabled:

- PostgreSQL uses `sslmode=verify-full`;
- MySQL uses `ssl-mode=VERIFY_IDENTITY`;
- MongoDB uses `tls=true`.

PostgreSQL/MySQL certificate and hostname verification use public trust roots. Custom/private/self-signed CA configuration, client certificates, SSH tunnels, and cloud-auth plugins are not supported in 0.1. TLS can be disabled explicitly for compatible local environments.

## Redrob AI egress

Desktop AI calls `https://console.redrob.ai/api/backend/v1/chat/completions` with model `auto`.

The renderer-facing request permits only:

- user prompt (maximum 8 KiB);
- optional active query (maximum 64 KiB);
- database kind;
- explicit query language.

Rust constructs the system instruction, model, temperature, and message shape. Unknown fields—including renderer-provided message arrays, result rows, credentials, model, or endpoint—are rejected/omitted. Responses are capped at 2 MiB. The API key is used as the HTTPS bearer credential and is not inserted into messages.

Database result rows and credentials are not automatically attached. Anything manually typed or pasted into the prompt or active query is sent. A nonempty `REDROB_API_KEY` environment value takes precedence over the keyring value.

## Error handling

Database and connection errors are sanitized and bounded before crossing IPC. URLs and embedded credentials are redacted. Debug formatting for connection and AI request models omits identity/contents.

## Recommended deployment posture

- Use least-privilege, read-only database accounts.
- Keep TLS verification enabled and use public-CA certificates in this preview.
- Do not use production credentials in browser demos or live integration tests.
- Treat connection profile metadata and query text as sensitive even though they are not passwords.
- Review prompt/query text before sending it to Redrob.
