//! Phase-0 spike: sync an Automerge document between two editors over HTTP —
//! the seed for `rinch-http` + the sync engine. Reachable at `/sync-spike`.
//!
//! Editor A collaborates (`start_collaboration_host`); its initial snapshot and
//! every local-edit delta are pushed to the server over `fetch` (bytes hex-encoded
//! through the existing JSON transport). "Pull into B" fetches the snapshot + delta
//! log and reconstructs them into editor B via `start_collaboration_guest` +
//! `collab_receive`, converging B to A. The server (`/api/sync/*`) is a dumb relay
//! — it never runs Automerge; the clients merge. Content is flat (collab scope).

use wasm_bindgen::prelude::*;

use rinch::prelude::*;
use rinch_core::Signal;
use rinch_web::{Editor, create_editor};
use serde::{Deserialize, Serialize};

use crate::api;

const DOC_ID: &str = "spike-doc";
const FLAT: &str = "<h1>Shared chapter</h1><p>Editor A is the source. Its <strong>Automerge</strong> snapshot and deltas travel to the server over HTTP; Pull reconstructs them in B.</p>";

const CSS: &str = r#"
.sync-spike { max-width: 980px; margin: 0 auto; padding: 24px; }
.sync-status { color: var(--rinch-color-dimmed); font-size: 13px; margin: 8px 0; }
.sync-bar { display: flex; gap: 8px; margin: 12px 0; }
.sync-bar button { padding: 6px 12px; border: 1px solid var(--rinch-color-border); background: var(--rinch-color-surface); color: var(--rinch-color-text); border-radius: 6px; cursor: pointer; }
.sync-cols { display: flex; gap: 20px; }
.sync-col { flex: 1; min-width: 0; }
.sync-col h3 { font-size: 13px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--rinch-color-dimmed); }
.sync-host { border: 1px solid var(--rinch-color-border); border-radius: 8px; padding: 8px 12px; min-height: 160px; background: var(--rinch-color-body); }
"#;

#[derive(Serialize)]
struct HexBody {
    hex: String,
}

#[derive(Deserialize)]
struct SyncState {
    snapshot: Option<String>,
    deltas: Vec<String>,
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn from_hex(s: &str) -> Vec<u8> {
    (0..s.len().saturating_sub(1))
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[component]
pub fn sync_spike_page() -> NodeHandle {
    let a = create_editor();
    let b = create_editor();
    let status: Signal<String> =
        Signal::new("Editor A is collaborating; its snapshot was pushed. Edit A, then Pull into B.".to_string());

    // Pull: fetch the relayed snapshot + deltas and converge B to A.
    let b_pull = b.clone();
    let pull = move || {
        let b = b_pull.clone();
        api::get::<SyncState>(&format!("/api/sync/{DOC_ID}"), move |result| {
            match result {
                Ok(state) => {
                    let Some(snap_hex) = state.snapshot else {
                        status.set("No snapshot on the server yet.".to_string());
                        return;
                    };
                    if b.start_collaboration_guest(&from_hex(&snap_hex), |_| {}).is_err() {
                        status.set("Guest restore failed.".to_string());
                        return;
                    }
                    let mut applied = 0usize;
                    for d in &state.deltas {
                        if b.collab_receive(&from_hex(d)) {
                            applied += 1;
                        }
                    }
                    status.set(format!(
                        "Pulled over HTTP: snapshot + {}/{} deltas applied — B converged to A.",
                        applied,
                        state.deltas.len()
                    ));
                }
                Err(e) => status.set(format!("Pull failed: {e:?}")),
            }
        });
    };

    let tree = rsx! {
        Fragment {
            style { {CSS} }
            div { class: "sync-spike",
                h2 { "Automerge sync over HTTP spike" }
                p { class: "sync-status", id: "sync-status", {move || status.get()} }
                div { class: "sync-bar",
                    button { id: "sync-pull", onclick: pull, "Pull into B" }
                }
                div { class: "sync-cols",
                    div { class: "sync-col",
                        h3 { "Editor A · source (edits push to server)" }
                        div { class: "sync-host", id: "sync-a",
                            Editor { editor: a.clone(), content: FLAT.to_string() }
                        }
                    }
                    div { class: "sync-col",
                        h3 { "Editor B · pulls from server" }
                        div { class: "sync-host", id: "sync-b",
                            Editor { editor: b.clone(), content: "".to_string() }
                        }
                    }
                }
            }
        }
    };

    // Post-mount: A projects its loaded doc onto a CRDT; push the snapshot, and
    // relay every subsequent local delta to the server over HTTP.
    match a.start_collaboration_host(move |delta| {
        let hex = to_hex(&delta);
        api::post::<_, serde_json::Value>(
            &format!("/api/sync/{DOC_ID}/delta"),
            &HexBody { hex },
            move |_result| {},
        );
    }) {
        Ok(snapshot) => {
            let hex = to_hex(&snapshot);
            api::post::<_, serde_json::Value>(
                &format!("/api/sync/{DOC_ID}/snapshot"),
                &HexBody { hex },
                move |_result| {},
            );
        }
        Err(e) => status.set(format!("Host projection failed (flat-scope only): {e:?}")),
    }

    tree
}
