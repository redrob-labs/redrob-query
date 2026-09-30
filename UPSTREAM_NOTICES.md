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
`scripts/check-upstream-paths.mjs` rejects any copied path matching `src-commercial`. Nothing is
taken from this upstream until that guard is in place and reverse-verified.

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
| _(none yet)_ | | | |

## Trademarks

"Redrob" and the Redrob logo are trademarks of Redrob and are not licensed by the GPL. Beekeeper
Studio's name and logo are likewise its own; nothing here uses them beyond this attribution.
