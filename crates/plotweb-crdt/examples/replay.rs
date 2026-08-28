//! Replay a persisted body document from a directory of CRDT blobs.
//!
//!     cargo run -p plotweb-crdt --example replay -- <dir> [--steps]
//!
//! `<dir>` holds the pieces recovered from local storage, applied in filename
//! order: an optional base `snapshot` first, then each `delta`. This is the same
//! reconstruction the app does when it reopens a chapter (`local_store.rs`: load
//! the snapshot, `collab_receive` every delta).
//!
//! With `--steps`, the text is materialized after *every* delta and each point
//! where it shrank is reported — that locates a deletion or an overwrite inside a
//! writing session, rather than just showing where the session ended.
//!
//! When the base snapshot is gone (swept by a later generation's pointer flip),
//! yrs holds the deltas as pending and materializes nothing. The `--salvage` mode
//! is for exactly that case: it lifts the inserted strings straight out of the
//! update bytes in sequence order, which reads as the text that was typed.

use std::path::PathBuf;

use yrs::types::ToJson;
use yrs::updates::decoder::Decode;
use yrs::{GetString, ReadTxn, Transact};

fn blobs_in(dir: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("could not read the blob directory")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .collect();
    // Snapshot first, then deltas in zero-padded sequence order.
    entries.sort_by_key(|p| {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        (!name.contains("snapshot"), name)
    });
    entries
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).expect("could not read a blob");
            (p, bytes)
        })
        .collect()
}

/// The document's text, as the editor would render it.
fn text_of_doc<T: ReadTxn>(txn: &T, content: &yrs::ArrayRef) -> String {
    let any = content.to_json(txn);
    let mut buf = String::new();
    any.to_json(&mut buf);
    let value: serde_json::Value = serde_json::from_str(&buf).unwrap_or(serde_json::Value::Null);
    let mut out = String::new();
    collect_text(&value, &mut out);
    out
}

fn collect_text(v: &serde_json::Value, out: &mut String) {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(s) = m.get("text").and_then(|t| t.as_str()) {
                out.push_str(s);
            }
            if let Some(kids) = m.get("content").and_then(|c| c.as_array()) {
                kids.iter().for_each(|k| collect_text(k, out));
            }
            if matches!(
                m.get("type").and_then(|t| t.as_str()),
                Some("paragraph") | Some("heading")
            ) {
                out.push_str("\n\n");
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|k| collect_text(k, out)),
        _ => {}
    }
}

/// Printable strings carried inside an update's insert operations.
///
/// A yrs update stores inserted text as plain UTF-8 runs. Recovering them needs no
/// base document, which is what makes this work when the snapshot is gone.
fn strings_in(update: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = Vec::new();
    for &b in update {
        if b == b'\n' || b == b'\t' || (0x20..0x7f).contains(&b) || b >= 0x80 {
            run.push(b);
        } else {
            if run.len() >= 3 {
                if let Ok(s) = String::from_utf8(run.clone()) {
                    out.push(s);
                }
            }
            run.clear();
        }
    }
    if run.len() >= 3 {
        if let Ok(s) = String::from_utf8(run) {
            out.push(s);
        }
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: replay <dir> [--steps|--salvage]");
    let mode = args.next().unwrap_or_default();
    let blobs = blobs_in(&dir);
    eprintln!("[{} blobs in {dir}]", blobs.len());

    if mode == "--salvage" {
        let mut text = String::new();
        for (_, bytes) in &blobs {
            for s in strings_in(bytes) {
                text.push_str(&s);
            }
        }
        eprintln!("[salvaged {} chars from update bytes]", text.chars().count());
        println!("{text}");
        return;
    }

    let doc = yrs::Doc::new();
    let content = doc.get_or_insert_array("content");
    let _meta = doc.get_or_insert_map("meta");

    let mut applied = 0usize;
    let mut failed = 0usize;
    let mut prev = 0usize;
    let mut best = (0usize, String::new(), String::new());

    for (path, bytes) in &blobs {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let update = match yrs::Update::decode_v1(bytes).or_else(|_| yrs::Update::decode_v2(bytes)) {
            Ok(u) => u,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        if doc.transact_mut().apply_update(update).is_err() {
            failed += 1;
            continue;
        }
        applied += 1;

        if mode == "--steps" {
            let txn = doc.transact();
            let text = text_of_doc(&txn, &content);
            let len = text.chars().count();
            if len + 100 < prev {
                eprintln!("  {name}: text SHRANK {prev} -> {len} chars  <-- lost here");
            }
            if len > best.0 {
                best = (len, text.clone(), name.clone());
            }
            prev = len;
        }
    }

    let txn = doc.transact();
    let text = text_of_doc(&txn, &content);
    eprintln!(
        "[applied {applied}, unreadable {failed}; final text {} chars]",
        text.chars().count()
    );
    if mode == "--steps" && best.0 > text.chars().count() {
        eprintln!("[high-water mark: {} chars at {}]", best.0, best.2);
        println!("{}", best.1.trim_end());
        return;
    }
    println!("{}", text.trim_end());
}
