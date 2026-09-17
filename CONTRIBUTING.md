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

One long-lived branch, `main`. Short-lived branches off it, merged by pull request.

- **`main`** is the trunk: no direct pushes, no force pushes, no deletion.
- **Working branches** are `<type>/<short-slug>`, e.g. `fix/history-restore`,
  `feat/mongo-explain`. Types: `feat`, `fix`, `chore`, `docs`, `test`, `refactor`, `perf`.
- Release tags are cut from `main`, formatted `v<major>.<minor>.<patch>`.

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

There are no workflows in this repository yet, so the checks above are the ones that exist: run them
locally before pushing. Adding the CI that runs them is welcome and is a good first contribution.
