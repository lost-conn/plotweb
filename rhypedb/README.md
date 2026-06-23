# PlotWeb metadata on rhypedb

PlotWeb is migrating its **metadata** store (ownership/auth/beta-reader rows)
from SQLite to [rhypedb](https://github.com/joeleaver/rhypedb). Manuscript
content, chapters, notes and version history stay in **git** (`crates/plotweb-git`).

- [`schema.rhype`](./schema.rhype) — the object schema (5 types: `User`, `Book`,
  `BetaLink`, `BetaFeedback`, `BetaReply`).

## Embedded, not a server

rhypedb is embedded **in-process** (like git and SQLite already are) — there is
no separate `rhypedb-server` to run, no port, and no network hop. The backend
opens an `Arc<Database>` at startup and calls it from `spawn_blocking` (the
engine is synchronous). The adapter lives in `crates/plotweb-server/src/rhype.rs`.

- `rhypedb-engine` / `rhypedb-query` / `rhypedb-schema` are path deps on the
  sibling rhypedb checkout (`../../personal/rhypedb`, pinned by commit like
  rinch in the Dockerfile).
- The schema SDL is `include_str!`-baked into the binary (like the SQL
  migrations) and passed to `Database::open`, so it always matches the code.
- Data dir: `RHYPEDB_DATA_DIR` (default `data/rhypedb`).

### Build caveat (and the upstream fix)

`rhypedb-engine` depends on `rhypedb-embed` **unconditionally**, which pulls the
ONNX Runtime / `fastembed` / `ort` stack — even though PlotWeb's schema has no
vector fields. So PlotWeb's build currently compiles and links ONNX (slower
Docker/CI, larger binary); it is **unused at runtime**. The clean fix is an
upstream change to rhypedb making `rhypedb-embed` an optional feature (gating
`engine::vectorizer` and the query executor's vector path) — tracked as a
separate PR. Until then we accept the weight.

## Design constraints (why the schema looks the way it does)

- **UUID is not the engine id.** rhypedb assigns an auto `u64` object id the
  caller can't set. PlotWeb keys everything by UUID v4, so each row carries its
  UUID in `uuid: String @unique` and is found via `filter(.uuid == "…")`, never
  `get(<int>)`. Updates/deletes are also addressed by the uuid filter
  (`filter(.uuid == "…").update({...})` / `.delete()` — verified working).
- **Foreign keys are indexed UUID strings**, not rhypedb relations — every query
  addresses rows by UUID, and relations link by the engine's `u64` id.
- **Cascade deletes live in the adapter.** SQLite `ON DELETE CASCADE`
  (feedback→link, replies→feedback, links→book) is reproduced by deleting
  children explicitly; the engine doesn't enforce it here.
- **No `ORDER BY`, no `count()`/`exists()` in the DSL.** Ordering (e.g. books by
  `created_at DESC`) and counting are done in the adapter over returned rows;
  ownership checks are `filter(...).limit(1)` + "non-empty?".
- **No NULL / no field defaults.** Nullable columns (`max_chapter_index`,
  `pinned_commit`, `user_id`) are simply absent when unset.
