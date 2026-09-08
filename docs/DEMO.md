# Demo guide

Redrob Data has two distinct demo experiences.

## Browser sample-data demo

Run:

```bash
npm ci
npm run dev
```

Open `http://127.0.0.1:1420`. This uses `DemoBridge`, not native drivers. The title bar says **Interactive browser workspace · sample data only**, and query tabs/history display **In-memory demo**.

### Demonstrable flow

1. Start on the connected **Acme Warehouse** PostgreSQL sample profile.
2. Expand `commerce → public → customers` or focus navigator search from the activity rail.
3. Run the starter query. The default page contains rows 1–50; use the next control for the remaining rows 51–64 or switch among 25/50/100/250 page sizes.
4. Sort or filter the current page, hide/show columns, inspect execution messages, and export the currently visible rows/columns to CSV.
5. Edit a supported relational sample cell, inspect staged changes, and apply it. The whole batch is checked against exact previous values before the profile's in-memory fixture changes.
6. Open local query history, restore the successful first-page query, or clear history. Browser history disappears on reload.
7. Ask Redrob AI for monthly revenue, insert the generated SQL, and run it.
8. Create, test, edit, disconnect/reconnect, and remove a user sample profile. Built-in profiles cannot be edited or removed.
9. Add a MongoDB profile with a target database and optional `authSource`; switch to its Mongo-owned MQL tab and run the deterministic read-only customer `find`.

Connection tests and lifecycle transitions are simulated; no host is contacted. Redrob responses are local deterministic fixtures. An entered AI key is neither stored nor sent. Added profiles, queries, history, and edits disappear when the page reloads.

Browser editing is intentionally limited to sample relational fixtures. Mongo sample results and all native desktop results are read-only. A stale or partly invalid mutation batch is rejected atomically.

## Desktop built-in demo

The native core always includes non-removable **Demo SQLite**, backed by `:memory:`. Desktop startup does not select or connect automatically. Select the profile and invoke **Connect**; the core then seeds four customers and five orders and exposes native SQLite metadata, guarded reads, typed rows, and paging.

Desktop results remain read-only and do not expose cell editing. Desktop query tabs/history can be restored locally, but restoration never auto-selects, connects, or runs a query.

## Recorded acceptance demo

Demo-recorder configuration and output are intentionally excluded by `.gitignore`. A validated run produces:

- `video/demo.mp4` — H.264 deliverable;
- `video/demo.webm` — original browser recording;
- per-step screenshots and accessibility snapshots;
- `action-log.json` and `review-report.md`.

A passing review requires every scenario assertion to pass, no unexpected console errors, no failed network response, and a decodable MP4. The recording is evidence only for the browser sample-data workflow; it is not evidence that external databases, the native WebKit shell, or Redrob were contacted.

## Live connector tests

The three external connector tests are marked `#[ignore]`. A default Rust run executes hermetic tests and reports those three as ignored; it never silently treats a missing service as a live pass.

Export all disposable URLs and explicitly run ignored tests:

```bash
export REDROB_TEST_POSTGRES_URL='postgresql://…'
export REDROB_TEST_MYSQL_URL='mysql://…'
export REDROB_TEST_MONGO_URL='mongodb://…'
export REDROB_TEST_MONGO_DATABASE='redrob_test' # optional
cargo test --locked -p redrob-core --all-features -- --ignored
```

Or run one named gate with `-- --ignored`. An explicitly selected gate fails immediately if its required URL is missing.

The PostgreSQL gate covers native scalar and parameter behavior. The MySQL gate covers scalars, unsigned values, BIT/TINYINT, parameters, and file-output rejection. The Mongo gate creates one UUID-named collection and covers find/aggregate pagination, explain modes, BSON fidelity, and bounded metadata sampling before dropping that collection.

Use only non-production databases and least-privilege credentials. Rust does not automatically load `.env` files.
