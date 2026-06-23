# PlotWeb metadata on rhypedb

PlotWeb is migrating its **metadata** store (ownership/auth/beta-reader rows)
from SQLite to [rhypedb](https://github.com/joeleaver/rhypedb). Manuscript
content, chapters, notes and version history stay in **git** (`crates/plotweb-git`).

- [`schema.rhype`](./schema.rhype) — the object schema (5 types: `User`, `Book`,
  `BetaLink`, `BetaFeedback`, `BetaReply`).

## Running the server

rhypedb runs as a separate process; the PlotWeb backend talks to it over HTTP
(`POST /query`). Build the server from the rhypedb checkout (a sibling repo,
pinned by commit like rinch):

```bash
# from the rhypedb checkout (../../personal/rhypedb relative to this repo)
cargo build -p rhypedb-server           # default features only — see caveat below

./target/debug/rhypedb-server \
    --schema  /path/to/plotweb/rhypedb/schema.rhype \
    --data-dir /var/lib/plotweb/rhypedb \
    --listen      127.0.0.1:4200 \
    --tcp-listen  127.0.0.1:4201
```

- HTTP query API: `127.0.0.1:4200` (default). The data plane (`POST /query`,
  `GET /schema`, `GET /health`) needs **no auth**; only `/admin/*` is gated by
  `RHYPEDB_ADMIN_TOKEN`.
- Quick check: `curl localhost:4200/schema` lists the five types.

### Build caveat

Only the **default** build works. `--no-default-features` currently fails
upstream (fastembed 5.14 API drift). The default build pulls ONNX Runtime /
fastembed / hf-hub — heavy, and unused by PlotWeb (we have no vector fields).
This weight is a real cost to factor into the jkbase/deploy decision.

## Design constraints (why the schema looks the way it does)

- **UUID is not the engine id.** rhypedb assigns an auto `u64` object id the
  caller can't set. PlotWeb keys everything by UUID v4, so each row carries its
  UUID in `uuid: String @unique` and is found via `filter(.uuid == "…")`, never
  `get(<int>)`.
- **Foreign keys are indexed UUID strings**, not rhypedb relations — every query
  addresses rows by UUID, and relations link by the engine's `u64` id.
- **Cascade deletes live in the adapter.** SQLite `ON DELETE CASCADE`
  (feedback→link, replies→feedback, links→book) is reproduced by deleting
  children explicitly; the engine doesn't enforce it here.
- **No `ORDER BY`, no `count()`/`exists()` in the DSL.** Ordering (e.g. books by
  `created_at DESC`) and counting are done in the adapter over the returned rows;
  ownership checks are `filter(...).limit(1)` + "non-empty?".
- **No NULL / no field defaults.** Nullable columns (`max_chapter_index`,
  `pinned_commit`, `user_id`) are simply absent when unset.
