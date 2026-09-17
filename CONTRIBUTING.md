# Contributing

**English** · [한국어](./CONTRIBUTING.ko.md)

Thanks for helping. This is the working agreement for the repository: how branches are named, what has
to be green before a merge, and two rules that are easy to break by accident.

## Two rules

**1. This is a clean implementation, not a DBeaver fork.** The workflow is informed by DBeaver
Community; the code is not derived from it. So no DBeaver source, no DBeaver branding, and no Eclipse
RCP, OSGi, JDBC, Java or JVM runtime enters this repository. A dependency that pulls a JVM in is a
review stop, not a detail.

**2. Version 0.1 is READ-ONLY on purpose.** The desktop workspace is guarded: it reads from
PostgreSQL, MySQL, SQLite and MongoDB and does not write. A change that adds a write path is a change
to the product's promise and needs to say so in the pull request title, not only in the diff.

Local storage has its own rule. Query text and tab or history metadata are stored; result rows,
database credentials and prompts are not. Query text can itself contain sensitive literals, which is
why the history menu offers **Stop saving and clear local data** and why that opt-out survives a
restart. Do not widen what is stored without saying what it now includes.

## Branch model

Two long-lived branches. `develop` is where work lands; `main` is what has been released.

- **`develop`** is the default branch and the integration branch. Cut working branches from it and
  open pull requests back into it. Clone the repository and you are on `develop`.
- **`main`** is the released state. It takes pull requests only from `release/*` and `hotfix/*`
  branches, and release tags are cut from it. Nothing else merges here.
- **Working branches** are `<type>/<short-slug>` off `develop`, e.g. `fix/history-restore`, `feat/mongo-explain`. Types: `feat`,
  `fix`, `chore`, `docs`, `test`, `refactor`, `perf`.
- **Merging is squash-only**, and the branch is deleted on merge. One pull request becomes one
  commit, so `git log develop` reads as a list of changes rather than a graph. The squash commit's
  body is the pull request body, not a concatenation of your work-in-progress messages.

Both branches are enforced by GitHub rulesets rather than by this document:

- no direct pushes — every change arrives as a pull request;
- no force pushes and no deletion of the branch;
- linear history;
- required status checks must pass — they do not have to pass against the newest tip, so a queue of
  bot updates does not have to rebase and re-run one at a time;
- review threads must be resolved before merge.

A ruleset cannot express *which* branch a pull request comes from, so "only `release/*` and
`hotfix/*` merge into `main`" is a convention this document carries and reviewers uphold. One part
of it is machine-checked: the release workflow refuses to build a tag whose commit is not reachable
from `origin/main`, so tagging straight off `develop` fails instead of shipping.

### Releasing

```bash
git switch develop && git pull
git switch -c release/v0.2.0
# bump the version, update the changelog, run the release check
# open a pull request into main and merge it, then tag main:
git switch main && git pull
git tag -a v0.2.0 -m "Redrob Query v0.2.0"
git push origin v0.2.0
# bring main's release commit back so develop does not fall behind:
git switch -c chore/sync-main-to-develop main
# open a pull request into develop
```

A hotfix is the same shape with `hotfix/*` cut from `main` rather than `develop`, and it merges into
both.

Fork the repository, push your branch to your fork, and open the pull request from there. You do not
need write access to contribute, and pull requests from forks run CI with no repository secrets.

## Day to day

```bash
git switch develop && git pull
git switch -c fix/short-description

npm install
npm run typecheck
npm run test
npm run build

cargo fmt --all
cargo clippy --all-targets -- -D warnings

npm run release:check   # the release validator, before tagging

# open a pull request into develop
```

The browser demo is backed by deterministic in-memory sample data, on purpose: it demonstrates the
product without a database and without anybody's credentials. Keep it deterministic, so a screenshot
taken today matches one taken next month.

### Commits

Subject in the imperative. Use the body to say _why_, and when a change touches what is stored locally
or what the app is allowed to write, say so there.

## Branding

The mark in `src/assets/` is the Redrob mark, the same one Redrob Cowork ships, and the app icons are
generated from it. Replacing it with a drawing that merely resembles it is how a product ends up with
two logos.

## What CI checks

`.github/workflows/ci.yml` runs on every pull request and on pushes to `main` and `develop`. It consumes no
secrets, so a pull request from a fork gets exactly the same run as one from a branch here. Two jobs,
and both are required before a merge:

| Check | What it runs |
| --- | --- |
| **Release metadata, types, tests, build** | `npm run release:check`, `npm run typecheck`, `npm test`, `npm run build` |
| **Format, clippy, tests** | `npm run rust:fmt`, `npm run rust:clippy` (`-D warnings`), `npm run rust:test` |

`npm run release:check` is the one worth knowing about. It pins the things that fan out across
several files and go wrong silently: the version in `package.json`, `Cargo.toml`, `tauri.conf.json`
and `package-lock.json`; the Cargo `rust-version` against `rust-toolchain.toml`; the Tauri
`productName` and `identifier`; and that every PNG under `src-tauri/icons/` is RGBA. The last one is
not cosmetic — `tauri::generate_context!` panics at compile time on a non-RGBA icon, so a single RGB
icon means the desktop app does not build.

`npm run check` runs the whole set locally in one command.

## Dependency updates

Dependabot **version updates are off**: they produced a standing queue of pull requests, each
needing its own CI run, and on this repository the churn cost more than it caught. Two things
replace them:

- **Dependabot security updates are on.** A dependency with a known advisory still gets a pull
  request opened automatically. That is the part worth interrupting for.
- **The advisory job gates every pull request.** `cargo audit` runs against the committed
  lockfile, so a vulnerable dependency cannot merge even if nobody read an alert. A security
  update is a notification; this is the control.

Routine bumps are therefore deliberate: bump what you need for the change you are making, in the
same pull request, and say why in the body. Do not sweep unrelated versions into a feature branch.
