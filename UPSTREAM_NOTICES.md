# Upstream source notices

Attribution for source code **copied into this repository** from another project.

Only upstreams whose `kind` is `code` in `docs/upstream-sources.toml` belong here. An `algorithm`
entry must not appear: reading a program to reimplement its behaviour creates no attribution duty,
and listing one would misstate what this repository contains. `scripts/verify-upstream.mjs` enforces
both directions.

## beekeeper

Beekeeper Studio Community Edition, Copyright (c) 2020-present Beekeeper Studio, Inc.

Pinned at `c67ca751f4a006b195d6a29d91a9a4f7b11e84b1` of
<https://github.com/beekeeper-studio/beekeeper-studio>.

Licensed under the GNU General Public License, "either version 3 of the License, or (at your option)
any later version" — so GPL-3.0-or-later. This product carries the same licence for that reason; it
was Apache-2.0 until this upstream was taken on. See `NOTICE`.

### The commercial boundary

The upstream repository is **mixed licence**. `LICENSE-COMMERCIAL.md` puts every file in any
`src-commercial` directory under a commercial licence that forbids use in a separate project; all
other files are GPLv3.

Measured at the pinned commit:

| | Files | Size |
|---|---|---|
| `src-commercial` — must never be copied | 55 | 544 KB |
| GPLv3 — portable | 1,817 | 284.1 MB |

55 of 1,872 is too small a fraction for review to catch reliably, so it is a machine check:
`scripts/check-upstream-paths.mjs`, run by `npm run check`. **It is in place, reverse-verified, and
it handles no commercial content.**

Four layers, because each catches a different mistake:

1. **Content.** Every tracked source file is hashed and compared against the 54 recorded git blob
   SHA-1 values in `docs/beekeeper-commercial-fingerprint.json`. Catches a verbatim copy, which is
   what a paste *is* at the moment it happens — and the check runs before adaptation can disguise
   it. Renaming the file does not help: a hash does not look at the name.
2. **Provenance.** The "Which subsystems" table below may not record a commercial upstream path.
3. **Working tree.** No tracked path may lie under `src-commercial`.
4. **The ignore rule itself.** `/upstream/` must actually be ignored by git, probed with a path
   inside it rather than the directory name, because the rule matches a directory and
   `git check-ignore upstream` misses it in a fresh clone where the directory does not exist.

The fingerprint records **only hashes**. They come from the upstream repository's git *tree*
listing, which publishes each blob's hash without its bytes, so nothing under `src-commercial` was
fetched to build it and nothing is stored here. A guard that embedded the material it protects
against would be the copy it exists to prevent.

The empty blob is deliberately excluded. `src-commercial/.keep` is an empty file and every empty
file shares its hash; recording it would flag every empty file in this repository. Excluded by hash
rather than by name, so a future empty commercial file is still only excluded for being empty.

### Views are translated, not copied

Beekeeper's views are Vue; this product is React. Those files are reimplemented rather than copied,
so they carry no attribution duty — but the logic, drivers and query layer beneath them are
TypeScript on both sides and **are** copied. Only the copied part is listed below.

Per GPL section 5(a), copied files carry a notice stating that they were changed and when.

### Which subsystems

Filled in as porting lands, one row per subsystem, so this file states what is actually here rather
than what was planned.

| Subsystem | Upstream path | Our path | Landed |
|---|---|---|---|
| SQLite key and index reading | `apps/studio/src/lib/db/clients/sqlite.ts` | `src/plugins/schemaKeys.ts` | 2026-09-30 |
| composite foreign key grouping | `apps/studio/src/lib/db/clients/postgresql.ts` | `src/plugins/schemaKeys.ts` | 2026-09-30 |
| PostgreSQL key and index reading | `apps/studio/src/lib/db/clients/postgresql.ts` | `src/plugins/postgresKeys.ts` | 2026-09-30 |

Both rows are one file, and the pairing is the point: the SQLite client supplies the pragma queries,
and the grouping treatment comes from the POSTGRES client because the SQLite one hardcodes
`isComposite: false` and so loses every multi-column foreign key. Three deliberate departures, each
verified against a real database rather than reasoned about:

1. Composite keys are grouped, as the postgres client does it. SQLite's pragma supplies the grouping
   key (`id`, with `seq` for column order) -- measured: a table with one composite and one simple key
   returns three rows with two distinct ids.
2. A constraint id is prefixed with its owning table when scanning for incoming keys, because pragma
   ids restart at 0 per table and two tables' first keys would otherwise group into one composite.
3. A table name is resolved against metadata the database reported before it reaches SQL. Upstream
   escapes and interpolates; query's `QueryRequest` has no bind-parameter channel at all, so an
   allow-list is the equivalent guarantee -- and the name arrives from untrusted plugin content.

The PostgreSQL port departs once more, and it is another defect rather than a preference: upstream
groups rows with `_.groupBy(rows, 'constraint_name')`, but PostgreSQL scopes a constraint name to its
TABLE, so two tables may both have a foreign key called `fk_same`. getIncomingKeys scans every table
referencing the target, so those two unrelated keys merge into one bogus composite. Measured on a live
PostgreSQL 18.6: five rows referencing one table carried three distinct connames and FOUR distinct
oids. The port groups by `c.oid`.

SQLite and PostgreSQL are ported. MySQL and SQL Server are not: their SQL is upstream and porting it is
mechanical, but there is no server for either on the development machine, and SQL that has never
executed is a guess that happens to compile. PostgreSQL was in that sentence until it turned out the
server runs fine from an extracted .deb as an unprivileged user -- so "no server here" was an
assumption, and docs/dialect-verification.md records how to reproduce the verification and what it
caught.

**Written here, not copied — so deliberately absent from the table above.** The plugin host in
`src/plugins/` implements the same wire protocol Beekeeper's host speaks, and it contains none of
their source. The envelope was measured from `@beekeeperstudio/plugin` 1.7.1 (MIT), which is the
plugin half of the conversation and is linked rather than copied, and the method semantics come from
1,272 lines of public reference documentation. A protocol is a specification, not a copyrightable
body of code, and the registry records it as such: `kind = "protocol"` for what we speak,
`kind = "library"` for the two packages we link.

When source is genuinely copied from Beekeeper it goes in the table above, with its pinned commit.

## Trademarks

"Redrob" and the Redrob logo are trademarks of Redrob and are not licensed by the GPL. Beekeeper
Studio's name and logo are likewise its own; nothing here uses them beyond this attribution.
