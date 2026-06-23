//! Embedded rhypedb adapter.
//!
//! PlotWeb's metadata store (ownership/auth/beta readers) runs on an *embedded*
//! rhypedb engine — `Arc<Database>` shared across handlers, called from
//! `spawn_blocking` (the engine is synchronous, like the git layer). Manuscript
//! content stays in git.
//!
//! rhypedb has no client crate and its query API is string-based, so this module
//! provides: a small connection wrapper ([`RhypeStore`]), a JSON view of a row
//! ([`RhypeObject`]), and DSL string-building helpers ([`quote`], [`Fields`])
//! with the escaping the parser expects. See `rhypedb/schema.rhype` and
//! `rhypedb/README.md` for the design constraints (UUID-as-field, no joins, no
//! ORDER BY/count — done in the adapter).

use std::path::Path;
use std::sync::Arc;

use rhypedb_engine::database::Database;
use rhypedb_engine::object::{value_to_query_json, Object};
use rhypedb_query::executor::{execute, ExecContext, QueryOutput};
use rhypedb_query::parser::parse_query;
use rhypedb_schema::parser::parse_schema;
use serde_json::{Map, Value};

/// Schema SDL baked into the binary so it always matches the code that queries
/// it (the same approach as the bundled SQL migrations).
const SCHEMA_SDL: &str = include_str!("../../../rhypedb/schema.rhype");

#[derive(Debug)]
pub enum RhypeError {
    /// Failed to parse the schema or open the data dir.
    Open(String),
    /// The query string failed to parse.
    Query(String),
    /// The engine rejected execution (incl. unique-constraint violations).
    Engine(String),
    /// A result shape we didn't expect (e.g. create returned nothing).
    Unexpected(String),
}

impl std::fmt::Display for RhypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RhypeError::Open(m) => write!(f, "rhypedb open error: {m}"),
            RhypeError::Query(m) => write!(f, "rhypedb query error: {m}"),
            RhypeError::Engine(m) => write!(f, "rhypedb engine error: {m}"),
            RhypeError::Unexpected(m) => write!(f, "rhypedb unexpected: {m}"),
        }
    }
}
impl std::error::Error for RhypeError {}

pub type RhypeResult<T> = Result<T, RhypeError>;

/// A row read back from rhypedb: the engine's auto `u64` id plus its scalar
/// fields as JSON (`value_to_query_json` renders DateTime→RFC3339, Bytes→base64,
/// etc.). PlotWeb keys on the `uuid` field, not this `id`.
#[derive(Debug, Clone)]
pub struct RhypeObject {
    pub id: u64,
    pub fields: Map<String, Value>,
}

impl RhypeObject {
    pub fn str(&self, key: &str) -> Option<&str> {
        self.fields.get(key).and_then(Value::as_str)
    }
    pub fn string(&self, key: &str) -> Option<String> {
        self.str(key).map(str::to_string)
    }
    pub fn bool(&self, key: &str) -> Option<bool> {
        self.fields.get(key).and_then(Value::as_bool)
    }
    pub fn i64(&self, key: &str) -> Option<i64> {
        self.fields.get(key).and_then(Value::as_i64)
    }
}

fn to_object(mut o: Object) -> RhypeObject {
    o.ensure_fields_deserialized();
    let mut fields = Map::new();
    for (k, v) in o.fields.iter() {
        fields.insert(k.clone(), value_to_query_json(v));
    }
    RhypeObject { id: o.id, fields }
}

/// Internal `Send` result of a query, converted off the engine types inside the
/// blocking task.
enum Out {
    Objects(Vec<RhypeObject>),
    One(RhypeObject),
    Done,
}

/// Handle to the embedded rhypedb engine. Cheap to clone (`Arc`).
#[derive(Clone)]
pub struct RhypeStore {
    db: Arc<Database>,
}

impl RhypeStore {
    /// Open (or create) the embedded DB at `data_dir` with the baked-in schema.
    pub fn open(data_dir: impl AsRef<Path>) -> RhypeResult<Self> {
        let schema = parse_schema(SCHEMA_SDL)
            .map_err(|e| RhypeError::Open(format!("schema parse: {e}")))?;
        let db = Database::open(schema, data_dir).map_err(|e| RhypeError::Open(e.to_string()))?;
        Ok(Self { db })
    }

    /// Open using `RHYPEDB_DATA_DIR` (default `data/rhypedb`).
    pub fn from_env() -> RhypeResult<Self> {
        let dir = std::env::var("RHYPEDB_DATA_DIR").unwrap_or_else(|_| "data/rhypedb".into());
        Self::open(dir)
    }

    /// Parse + execute a query string on the blocking pool (the engine is sync).
    async fn run(&self, query: String) -> RhypeResult<Out> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let q = parse_query(&query).map_err(|e| RhypeError::Query(e.to_string()))?;
            let ctx = ExecContext::new(&db, None);
            let out = execute(&ctx, &q).map_err(|e| RhypeError::Engine(e.to_string()))?;
            Ok(match out {
                QueryOutput::Objects(v) => Out::Objects(v.into_iter().map(to_object).collect()),
                QueryOutput::Single(o) => Out::One(to_object(o)),
                QueryOutput::Done => Out::Done,
                // execute() materializes IdSet variants to Objects before
                // returning, so these shouldn't occur on the terminal result.
                _ => return Err(RhypeError::Unexpected("non-terminal query output".into())),
            })
        })
        .await
        .map_err(|e| RhypeError::Engine(format!("task join failed: {e}")))?
    }

    /// Run a query expected to return rows (get/filter/scan/traverse).
    pub async fn find(&self, query: impl Into<String>) -> RhypeResult<Vec<RhypeObject>> {
        match self.run(query.into()).await? {
            Out::Objects(v) => Ok(v),
            Out::One(o) => Ok(vec![o]),
            Out::Done => Ok(vec![]),
        }
    }

    /// Like [`find`](Self::find) but returns the first row, if any.
    pub async fn find_one(&self, query: impl Into<String>) -> RhypeResult<Option<RhypeObject>> {
        Ok(self.find(query).await?.into_iter().next())
    }

    /// True if the query returns at least one row. Use with
    /// `Type.filter(...).limit(1)` for ownership/existence checks (the DSL has no
    /// `count()`/`exists()`).
    pub async fn exists(&self, query: impl Into<String>) -> RhypeResult<bool> {
        Ok(!self.find(query).await?.is_empty())
    }

    /// Run a `Type.create({...})` and return the created row.
    pub async fn create(&self, query: impl Into<String>) -> RhypeResult<RhypeObject> {
        match self.run(query.into()).await? {
            Out::One(o) => Ok(o),
            Out::Objects(mut v) if !v.is_empty() => Ok(v.swap_remove(0)),
            _ => Err(RhypeError::Unexpected("create returned no object".into())),
        }
    }

    /// Run a query for its effect (update/delete/link), ignoring any result.
    pub async fn exec(&self, query: impl Into<String>) -> RhypeResult<()> {
        self.run(query.into()).await.map(|_| ())
    }
}

/// Quote and escape `s` as a rhypedb DSL string literal (including the surrounding
/// quotes). Escapes match the parser: `\`, `"`, newline, tab, carriage return.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Builder for a `{ key: value, ... }` object literal used in `create`/`update`.
/// Field names are PlotWeb's own constants (not user input), so they're written
/// verbatim; values are typed and escaped. Absent optionals are skipped — which
/// is exactly how rhypedb represents a NULL column.
#[derive(Default)]
pub struct Fields {
    parts: Vec<String>,
}

impl Fields {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn str(mut self, key: &str, value: &str) -> Self {
        self.parts.push(format!("{key}: {}", quote(value)));
        self
    }
    pub fn opt_str(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.str(key, v),
            None => self,
        }
    }
    pub fn bool(mut self, key: &str, value: bool) -> Self {
        self.parts.push(format!("{key}: {value}"));
        self
    }
    pub fn int(mut self, key: &str, value: i64) -> Self {
        self.parts.push(format!("{key}: {value}"));
        self
    }
    pub fn opt_int(self, key: &str, value: Option<i64>) -> Self {
        match value {
            Some(v) => self.int(key, v),
            None => self,
        }
    }
    /// Set a field to `null` — used by updates that clear a column (e.g.
    /// detaching a beta link's user, unpinning a commit). Reads of a null field
    /// come back as absent (`None`).
    pub fn null(mut self, key: &str) -> Self {
        self.parts.push(format!("{key}: null"));
        self
    }
    /// Render the `{ ... }` literal.
    pub fn render(&self) -> String {
        format!("{{ {} }}", self.parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_dsl_specials() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("a\\b"), "\"a\\\\b\"");
        assert_eq!(quote("line1\nline2\tx"), "\"line1\\nline2\\tx\"");
    }

    #[test]
    fn fields_renders_typed_literal() {
        let f = Fields::new()
            .str("uuid", "u-1")
            .str("title", "He said \"hi\"")
            .bool("active", true)
            .int("max_chapter_index", 3);
        assert_eq!(
            f.render(),
            "{ uuid: \"u-1\", title: \"He said \\\"hi\\\"\", active: true, max_chapter_index: 3 }"
        );
    }

    #[test]
    fn fields_skips_absent_optionals() {
        let f = Fields::new()
            .str("uuid", "u-1")
            .opt_str("pinned_commit", None)
            .opt_int("max_chapter_index", None);
        assert_eq!(f.render(), "{ uuid: \"u-1\" }");
    }

    fn user_create(uuid: &str, username: &str, email: &str) -> String {
        format!(
            "User.create({})",
            Fields::new()
                .str("uuid", uuid)
                .str("username", username)
                .str("email", email)
                .str("password_hash", "h")
                .str("created_at", "2026-01-01 00:00:00")
                .render()
        )
    }

    /// Full round-trip against a real embedded engine: open with the baked-in
    /// schema, create, filter-by-uuid, ownership-style exists, unique
    /// enforcement, and — importantly for the route rewrites — update and delete
    /// addressed by `filter(.uuid == …)` rather than the engine's integer id.
    #[tokio::test]
    async fn embedded_round_trip() {
        let dir = std::env::temp_dir().join(format!("pw_rhype_it_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = RhypeStore::open(&dir).expect("open store");

        let created = store.create(user_create("u-1", "alice", "a@e.com")).await.expect("create");
        assert_eq!(created.str("uuid"), Some("u-1"));

        let got = store
            .find_one(format!("User.filter(.uuid == {}).limit(1)", quote("u-1")))
            .await
            .expect("find");
        assert_eq!(got.and_then(|o| o.string("username")).as_deref(), Some("alice"));

        assert!(store.exists("User.filter(.uuid == \"u-1\").limit(1)").await.unwrap());
        assert!(!store.exists("User.filter(.uuid == \"nope\").limit(1)").await.unwrap());

        // @unique on uuid is enforced.
        assert!(store.create(user_create("u-1", "bob", "b@e.com")).await.is_err());

        // update + delete addressed by uuid filter (not get(<int id>)).
        store
            .exec(format!(
                "User.filter(.uuid == {}).update({})",
                quote("u-1"),
                Fields::new().str("username", "alice2").render()
            ))
            .await
            .expect("update");
        let updated = store
            .find_one("User.filter(.uuid == \"u-1\").limit(1)")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.str("username"), Some("alice2"));

        store
            .exec(format!("User.filter(.uuid == {}).delete()", quote("u-1")))
            .await
            .expect("delete");
        assert!(!store.exists("User.filter(.uuid == \"u-1\").limit(1)").await.unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
