# Demo guide

Redrob Data has two different demo experiences.

## Browser sample-data demo

Run:

```bash
npm ci
npm run dev
```

This uses `DemoBridge`, not native database drivers.

### Demonstrable flow

1. Open the Acme Warehouse PostgreSQL sample profile.
2. Expand `commerce → public → customers` in the navigator.
3. Run the starter query and filter/export the typed result grid.
4. Edit a supported relational sample cell, inspect staged changes, and apply it to that profile's in-memory fixture.
5. Ask Redrob AI for monthly revenue, insert the generated SQL, and run it.
6. Add a MongoDB demo profile with a target database and optional `authSource`.
7. Switch to the Mongo-owned MQL tab and run the deterministic read-only customer `find`.

The banner explicitly says sample data only. Connection tests are simulated; no host is contacted. Redrob responses are local deterministic fixtures. An entered AI key is neither stored nor sent. Added profiles and edits disappear on reload.

## Desktop built-in demo

The native core always includes a non-removable `Demo SQLite` profile backed by `:memory:`. On explicit selection/connect, it creates four customer and five order rows. This exercises the native SQLite path, metadata loading, read policy, and typed results.

Desktop results remain read-only and do not expose demo cell editing.

## Recorded acceptance demo

The repository's demo-recorder configuration is intentionally ignored by Git. A validated run produces:

- `video/demo.mp4` — H.264 deliverable;
- `video/demo.webm` — original Playwright recording;
- per-step screenshots and accessibility snapshots;
- `action-log.json` and `review-report.md`.

A passing review requires every scenario assertion to pass and no unexpected console or failed-network response. The recording is evidence for the browser sample-data workflow, not evidence that external database servers or Redrob were contacted.

## Live connector tests

External connectors are tested only when disposable URLs are explicitly exported:

```bash
export REDROB_TEST_POSTGRES_URL='postgresql://…'
export REDROB_TEST_MYSQL_URL='mysql://…'
export REDROB_TEST_MONGO_URL='mongodb://…'
export REDROB_TEST_MONGO_DATABASE='redrob_test' # optional
cargo test -p redrob-core --all-features live_
```

The PostgreSQL gate covers native scalar/parameter behavior. The MySQL gate covers scalars, unsigned values, BIT/TINYINT, parameters, and file-output rejection. The Mongo gate creates a UUID-named collection and covers find/aggregate pagination, explain modes, and BSON fidelity before dropping that collection.

If the variables are absent, the live test bodies return early; a green default test run must not be presented as live-server evidence.
