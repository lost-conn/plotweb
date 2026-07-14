//! Phase-0 spike: persist an Automerge snapshot in the browser and restore it
//! across a page reload — the seed for the `rinch-storage` crate. Reachable at
//! `/opfs-spike`.
//!
//! Flow: the rinch editor (model-first) holds a document; "Save" projects it onto
//! an Automerge CRDT via `EditorHandle::start_collaboration_host` (the collab byte
//! seam, behind rinch-web's `collaboration` feature) and persists the snapshot
//! bytes. On load, if a snapshot exists, it's read back and adopted via
//! `start_collaboration_guest`. Content is deliberately **flat**
//! (headings/paragraphs/marks) — the staged collab projection scope.
//!
//! ## Storage: localStorage, not OPFS (spike finding)
//! The intended backend is OPFS, but OPFS's write handles
//! (`FileSystemFileHandle`/`FileSystemWritableFileStream`) are behind web-sys's
//! `web_sys_unstable_apis` cfg — and enabling that cfg globally **fails to compile
//! rinch**: `rinch/src/render_surface.rs:1312` calls
//! `put_image_data(&img, 0.0, 0.0)` with floats where the unstable web-sys
//! signature wants `i32`. So OPFS via web-sys is blocked until rinch fixes that
//! path (a one-liner) OR `rinch-storage` binds OPFS through manual
//! wasm-bindgen/js-sys instead of web-sys (avoiding the global cfg). For this
//! spike we prove the round-trip with stable `localStorage` (bytes hex-encoded);
//! the Automerge + persistence + reload path is identical — only the sink differs.

use wasm_bindgen::prelude::*;

use rinch::prelude::*;
use rinch_core::Signal;
use rinch_web::{Editor, create_editor};

const STORE_KEY: &str = "plotweb-spike-doc";
const FLAT_SAMPLE: &str = "<h1>Chapter One</h1><p>The lantern <strong>guttered</strong> against the <em>fog</em> while the harbour bell counted out the hours.</p><p>Edit this, click <strong>Save</strong>, then reload the page — the Automerge snapshot is restored.</p>";

const SPIKE_CSS: &str = r#"
.opfs-spike { max-width: 820px; margin: 0 auto; padding: 24px; }
.opfs-spike-bar { display: flex; gap: 8px; align-items: center; margin: 12px 0; }
.opfs-spike-bar button { padding: 6px 12px; border: 1px solid var(--rinch-color-border); background: var(--rinch-color-surface); color: var(--rinch-color-text); border-radius: 6px; cursor: pointer; }
.opfs-status { color: var(--rinch-color-dimmed); font-size: 13px; }
.opfs-editor-host { border: 1px solid var(--rinch-color-border); border-radius: 8px; padding: 8px 12px; min-height: 200px; background: var(--rinch-color-body); }
"#;

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
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
pub fn opfs_spike_page() -> NodeHandle {
    let editor = create_editor();
    let status: Signal<String> = Signal::new("Loading…".to_string());

    // Save: project the current doc onto a CRDT and persist the snapshot.
    let ed_save = editor.clone();
    let save = move || {
        match ed_save.start_collaboration_host(|_| {}) {
            Ok(snapshot) => {
                let n = snapshot.len();
                match local_storage() {
                    Some(ls) => match ls.set_item(STORE_KEY, &to_hex(&snapshot)) {
                        Ok(()) => status.set(format!("Saved {n} bytes. Reload the page →")),
                        Err(e) => status.set(format!("Persist failed: {e:?}")),
                    },
                    None => status.set("localStorage unavailable".to_string()),
                }
            }
            Err(e) => status.set(format!("Snapshot failed (flat-scope only): {e:?}")),
        }
    };

    let tree = rsx! {
        Fragment {
            style { {SPIKE_CSS} }
            div { class: "opfs-spike",
                h2 { "Automerge persistence spike (localStorage stand-in for OPFS)" }
                p { class: "opfs-status", id: "opfs-status", {move || status.get()} }
                div { class: "opfs-spike-bar",
                    button { id: "opfs-save", onclick: save, "Save" }
                }
                div { class: "opfs-editor-host", id: "opfs-editor-host",
                    Editor {
                        editor: editor.clone(),
                        content: FLAT_SAMPLE.to_string(),
                    }
                }
            }
        }
    };

    // Post-mount: if a snapshot was persisted on a prior visit, restore it.
    let restored = local_storage().and_then(|ls| ls.get_item(STORE_KEY).ok().flatten());
    match restored {
        Some(hex) if !hex.is_empty() => {
            let bytes = from_hex(&hex);
            let n = bytes.len();
            match editor.start_collaboration_guest(&bytes, |_| {}) {
                Ok(()) => status.set(format!("Restored {n} bytes — this is your saved document.")),
                Err(e) => status.set(format!("Restore failed: {e:?}")),
            }
        }
        _ => status.set("No saved document yet — showing default. Edit, then Save.".to_string()),
    }

    tree
}
