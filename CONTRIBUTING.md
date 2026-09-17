# Contributing

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

One long-lived branch, `main`. Short-lived branches off it, merged by pull request. That is the whole
model — there is no `develop`, no release branch and no long-lived integration branch to keep in sync.

- **`main`** is the trunk. A GitHub ruleset enforces it rather than trusting this document:
  - no direct pushes — every change arrives as a pull request;
  - no force pushes and no deletion of the branch;
  - linear history, so `main` reads as a list of changes rather than a graph;
  - required status checks must pass — they do not have to pass against the newest
    `main`, so a queue of bot updates does not have to rebase and re-run one at a time;
  - review threads must be resolved before merge.
- **Working branches** are `<type>/<short-slug>`, e.g. `fix/history-restore`,
  `feat/mongo-explain`. Types: `feat`, `fix`, `chore`, `docs`, `test`, `refactor`, `perf`.
- **Merging is squash-only**, and the branch is deleted on merge. One pull request becomes one commit
  on `main`, so `git log main` is the changelog. The squash commit's body is the pull request body,
  not a concatenation of your work-in-progress messages.
- Release tags are cut from `main`, formatted `v<major>.<minor>.<patch>`.

Fork the repository, push your branch to your fork, and open the pull request from there. You do not
need write access to contribute, and pull requests from forks run CI with no repository secrets.

[redrob-code](https://github.com/redrob-labs/redrob-code) runs Git Flow with a `develop` branch.
This one does not, so cut from `main`.

## Day to day

```bash
git switch main && git pull
git switch -c fix/short-description

npm install
npm run typecheck
npm run test
npm run build

cargo fmt --all
cargo clippy --all-targets -- -D warnings

npm run release:check   # the release validator, before tagging

# open a pull request into main
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

`.github/workflows/ci.yml` runs on every pull request and on pushes to `main`. It consumes no
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
