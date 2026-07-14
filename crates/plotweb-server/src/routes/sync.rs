//! Phase-0 spike: a **dumb** Automerge sync relay (in-memory, no auth).
//!
//! Stores one base snapshot + an append-only delta log per doc id, as hex. The
//! server does NOT run Automerge — it just relays opaque CRDT bytes between
//! clients, which do the merging. This validates the HTTP transport for the sync
//! engine (the real server will persist canonical Automerge, but the transport
//! shape is the same). Not for production; state is process-local and unbounded.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use axum::Json;
use axum::extract::Path;
use serde::{Deserialize, Serialize};

#[derive(Default)]
struct DocRelay {
    snapshot: Option<String>,
    deltas: Vec<String>,
}

static RELAY: LazyLock<Mutex<HashMap<String, DocRelay>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize)]
pub struct HexBody {
    pub hex: String,
}

#[derive(Serialize)]
pub struct SyncState {
    pub snapshot: Option<String>,
    pub deltas: Vec<String>,
}

/// Set the base snapshot for a doc (resets the delta log).
pub async fn put_snapshot(Path(id): Path<String>, Json(body): Json<HexBody>) -> Json<serde_json::Value> {
    let mut map = RELAY.lock().unwrap();
    let entry = map.entry(id).or_default();
    entry.snapshot = Some(body.hex);
    entry.deltas.clear();
    Json(serde_json::json!({ "ok": true }))
}

/// Append a local delta to a doc's log.
pub async fn post_delta(Path(id): Path<String>, Json(body): Json<HexBody>) -> Json<serde_json::Value> {
    let mut map = RELAY.lock().unwrap();
    map.entry(id).or_default().deltas.push(body.hex);
    Json(serde_json::json!({ "ok": true }))
}

/// Fetch the current base snapshot + all deltas for a doc.
pub async fn get_state(Path(id): Path<String>) -> Json<SyncState> {
    let map = RELAY.lock().unwrap();
    let d = map.get(&id);
    Json(SyncState {
        snapshot: d.and_then(|d| d.snapshot.clone()),
        deltas: d.map(|d| d.deltas.clone()).unwrap_or_default(),
    })
}
