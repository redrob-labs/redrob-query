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
git switch -c release/0.4.0
# bump the version in Cargo.toml, write the CHANGELOG section, run the gate
# open a pull request into develop and merge it; then a pull request develop -> main
# (merge commit) and tag main:
git switch main && git pull
git tag -a v0.4.0 -m "Redrob Query v0.4.0"
git push origin refs/tags/v0.4.0:refs/tags/v0.4.0
gh workflow run release.yml --ref main -f tag=v0.4.0
# then merge main back into develop
```

The tag must equal the workspace version in `Cargo.toml` and `CHANGELOG.md` must carry a
`## <version>` section; the release is **published** when every platform leg has built, signed and
verified -- there is no draft to approve. To exercise the pipeline without releasing, run the
**Release** workflow with `dry_run=true`: it builds and signs the dispatched ref and publishes nothing.

A hotfix is the same shape with `hotfix/*` cut from `main` rather than `develop`, and it merges into
both.

Fork the repository, push your branch to your fork, and open the pull request from there. You do not
need write access to contribute, and pull requests from forks run CI with no repository secrets.

## Day to day

```bash
git switch develop && git pull
git switch -c fix/short-description

cargo fmt --all
cargo clippy -p redrob-query -p redrob-query-core --all-targets -- -D warnings
cargo test -p redrob-query-core
python3 scripts/check-brand.py

cargo run -p redrob-query     # the app; the built-in Demo SQLite profile needs no server

# open a pull request into develop
```

The built-in Demo SQLite profile is an in-memory database with deterministic sample data, on
purpose: it demonstrates the product without a server and without anybody's credentials. Keep it
deterministic, so a screenshot taken today matches one taken next month.

### Commits

Subject in the imperative. Use the body to say _why_, and when a change touches what is stored locally
or what the app is allowed to write, say so there.

## Branding

Every brand file (`resources/icons`, `resources/brand`, the installers' `res/`) is built from the
design system's published files by `scripts/build-brand-assets.py` and pinned by sha256 in
`brand/SOURCES.json`. Replacing one with a drawing that merely resembles it is how a product ends up
with two logos, and `scripts/check-brand.py` refuses it.

## What CI checks

`.github/workflows/ci.yml` runs on every pull request. It consumes no secrets, so a pull request from
a fork gets exactly the same run as one from a branch here. Two jobs:

| Check | What it runs |
| --- | --- |
| **brand assets** | `python3 scripts/check-brand.py` |
| **fmt, clippy, test (profile ci)** | `cargo fmt --all -- --check`, `cargo clippy -p redrob-query -p redrob-query-core --all-targets --locked --profile ci -- -D warnings`, `cargo test` of the same crates. Skipped when no Rust input changed or when the pull request targets `main` |

The release workflow (`.github/workflows/release.yml`) repeats only the brand check and
`cargo fmt --check`, then builds, signs and publishes the three targets.

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
