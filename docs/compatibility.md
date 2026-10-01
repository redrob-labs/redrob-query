# Compatibility matrix

Status: `planned`, `native`, `adapter`, `parity`.

This file did not exist until stage 3 of the porting plan asked for rows to be promoted to `parity` and there
was nothing to promote them in. `docs/DBEAVER-PORTING-MAP.md` maps DBeaver Community *workflows* onto this
product's equivalents, which is a different question and a different authority — DBeaver is registered
`kind = "algorithm"`, informing what a feature should do, and no line of it is copied. The authority for
**parity** is Beekeeper Studio, the one `kind = "code"` upstream.

**`parity` is not a judgement, it is a citation.** A row may claim it only by naming, in its Evidence column, a
harness that runs the authority — its own SQL, read from the pinned source, or a live server it must agree with
— and compares this product against it. `scripts/check-compatibility-matrix.mjs` checks both directions: a
`parity` row with no evidence and an evidence key naming no harness are each an error. `native` means only that
an implementation is here.

The harnesses a row may cite:

| Key | What it compares | Runs |
|---|---|---|
| `dialect:<check>` | this product's SQL against a live PostgreSQL or MySQL | `scripts/verify-dialect-sql.mjs` |
| `upstream-sql:<claim>` | the UPSTREAM's own SQL, read from the pinned file, against a live server | `scripts/verify-upstream-sql.mjs` |

Both need servers a contributor may not have, so neither is in `npm test` — a test that silently passes when its
subject is absent is worse than no test. `docs/dialect-verification.md` carries the recipe for starting both
from a private prefix without root.

**`Works` is a separate question: can a user reach it and does it do the job?** `native` shipped on rows no button
reaches (see `docs/user-paths.md`). `Works: yes` requires an `e2e:<path>::<name>` citation in Evidence naming a
test that drives the feature the way a user does; the guard checks the file exists and contains that name, and
refuses `yes` on a `planned` row. Every row starts at `no` and is promoted only with that evidence.

| Domain | Feature | Authority | Route | Status | Evidence | Works |
|---|---|---|---|---|---|---|
| Keys | SQLite primary keys, foreign keys in both directions, and indexes | Beekeeper `sqlite.ts` | ported into `src/schema/schemaKeys.ts`; composite keys are grouped rather than hardcoded false | parity | `upstream-sql:sqlite-iscomposite`, `dialect:sqlite-keys` | no |
| Keys | PostgreSQL keys and indexes, grouped by `pg_constraint.oid` | Beekeeper `postgresql.ts` | ported into `src/schema/postgresKeys.ts`; the upstream groups by constraint NAME, which merges keys PostgreSQL scopes per table | parity | `upstream-sql:postgres-grouping`, `upstream-sql:postgres-name-collision`, `dialect:postgres-keys` | no |
| Keys | MySQL keys and indexes | Beekeeper `mysql.ts` | ported into `src/schema/mysqlKeys.ts`; the join condition is written once and used in both directions | parity | `upstream-sql:mysql-on-clause`, `upstream-sql:mysql-row-count`, `upstream-sql:mysql-reference-actions`, `dialect:mysql-keys` | no |
| Keys | SQL Server, Oracle, and the rest of Beekeeper's dialect set | Beekeeper dialect clients | no client in this product; SQL Server was checked and is genuinely unavailable on this machine | planned | | no |
| Keys | MongoDB | — | MongoDB has no foreign keys; `DIALECTS_WITHOUT_KEYS` records it so the host refuses rather than answering empty | native | | no |
| Plugins | the plugin request/response envelope, 12 answered methods, the rest refused | `@beekeeperstudio/plugin` 1.7.1, MIT | `src/plugins/protocol.ts` and `host.ts`, measured from the SDK's own listener | native | | no |
| Plugins | manifest validation and a sandboxed iframe view | Beekeeper plugin manifest schema | `src/plugins/manifest.ts`, `PluginView.tsx`; `allow-scripts` WITHOUT `allow-same-origin` | native | | no |
| Plugins | the ER diagram, as a shipped GPL-3.0 plugin | `@beekeeperstudio/bks-er-diagram` 1.1.2 | installed into `public/plugins/` by `scripts/install-plugin-assets.mjs`, with an in-memory Storage shim so the sandbox can stay | native | | no |
| Structure | a table's keys and indexes shown in the UI | Beekeeper's structure tab | `src/components/StructurePanel.tsx`, with the schema name threaded down the tree | native | | no |
| Editing | insert, update, delete, DDL generation, transactions | Beekeeper | query 0.1 is documented read-only, so three of Beekeeper's five roles have nothing to act on; recorded as measurement 6 in `docs/beekeeper-porting-boundary.md` and explicitly NOT irreversible | planned | | no |
| Commercial | anything under Beekeeper's `src-commercial` | — | refused by a four-layer guard comparing git blob SHA-1 values; 55 commercial files, 544 KB, never fetched | native | | no |
