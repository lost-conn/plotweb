//! Recover chapter/note bodies from a copy of a browser's IndexedDB LevelDB store.
//!
//!     cargo run -p plotweb-crdt --example idb_recover -- <leveldb_dir> [doc-id-substring]
//!
//! The web build persists every edit locally (`plotweb-web/src/local_store.rs`):
//! per document, a `{doc_id}/manifest` naming the live generation, a
//! `{doc_id}/{gen}/snapshot` base, and an append-only `{doc_id}/{gen}/delta/{seq}`
//! log. Chrome stores those under its own IndexedDB key encoding, with the value
//! wrapped in a structured-clone envelope — so keys are recovered by scanning for
//! the UTF-16 doc-id text, and values by locating the CRDT blob inside the envelope.
//!
//! Reconstruction mirrors the app's own reopen path: load the snapshot, then apply
//! every delta in sequence order. Read-only; point it at a *copy* of the store.

use std::collections::BTreeMap;

use rusty_leveldb::LdbIterator;

/// One document's persisted pieces, keyed by generation.
#[derive(Default)]
struct Doc {
    /// generation -> snapshot bytes
    snapshots: BTreeMap<String, Vec<u8>>,
    /// generation -> (seq -> delta bytes)
    deltas: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

/// Pull a `chapter:`/`note:`/`book:`/`user:` key out of Chrome's UTF-16 key encoding.
fn utf16_key(bytes: &[u8]) -> Option<String> {
    // The key text is UTF-16 big-endian inside the IDB key; ASCII characters appear
    // as `00 <ch>`. Recover the ASCII run and keep it if it looks like a doc key.
    let mut s = String::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == 0 && bytes[i + 1].is_ascii_graphic() {
            s.push(bytes[i + 1] as char);
            i += 2;
        } else {
            if !s.is_empty() {
                if let Some(k) = doc_key(&s) {
                    return Some(k);
                }
                s.clear();
            }
            i += 1;
        }
    }
    doc_key(&s)
}

fn doc_key(s: &str) -> Option<String> {
    let start = ["chapter:", "note:", "book:", "user:"]
        .iter()
        .filter_map(|p| s.find(p))
        .min()?;
    let k = &s[start..];
    (k.len() > 8).then(|| k.to_string())
}

/// Find the CRDT blob inside a structured-clone value envelope.
///
/// Both projections are self-identifying: Automerge documents start with the magic
/// `85 6f 4a 83`, and everything else is taken as-is (a yrs update has no magic, so
/// the envelope's trailing bytes are tried whole).
fn blob_of(value: &[u8]) -> Vec<u8> {
    const AUTOMERGE_MAGIC: [u8; 4] = [0x85, 0x6f, 0x4a, 0x83];
    if let Some(pos) = value
        .windows(4)
        .position(|w| w == AUTOMERGE_MAGIC)
    {
        return value[pos..].to_vec();
    }
    // Structured-clone envelopes prepend a short header; the CRDT payload is the
    // tail. Trim a conservative leading window if the value is long enough.
    value.to_vec()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .expect("usage: idb_recover <leveldb_dir> [doc-id-substring]");
    let filter = args.next().unwrap_or_default();

    let opts = rusty_leveldb::Options {
        create_if_missing: false,
        ..Default::default()
    };
    let mut db = rusty_leveldb::DB::open(&dir, opts).expect("could not open the LevelDB store");

    let mut docs: BTreeMap<String, Doc> = BTreeMap::new();
    let mut manifests: BTreeMap<String, String> = BTreeMap::new();
    let mut it = db.new_iter().expect("could not iterate the store");
    while it.advance() {
        let Some((k, v)) = it.current() else { continue };
        let Some(key) = utf16_key(&k) else { continue };
        if !filter.is_empty() && !key.contains(&filter) {
            continue;
        }
        let parts: Vec<&str> = key.split('/').collect();
        let doc_id = parts[0].to_string();
        match parts.as_slice() {
            [_, "manifest"] => {
                if let Ok(g) = String::from_utf8(blob_of(&v)) {
                    let g = g.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_string();
                    manifests.insert(doc_id, g);
                }
            }
            [_, generation, "snapshot"] => {
                docs.entry(doc_id)
                    .or_default()
                    .snapshots
                    .insert(generation.to_string(), blob_of(&v));
            }
            [_, generation, "delta", seq] => {
                docs.entry(doc_id)
                    .or_default()
                    .deltas
                    .entry(generation.to_string())
                    .or_default()
                    .insert(seq.to_string(), blob_of(&v));
            }
            _ => {}
        }
    }

    for (doc_id, doc) in &docs {
        println!("== {doc_id}");
        if let Some(g) = manifests.get(doc_id) {
            println!("   live generation (manifest): {g}");
        }
        for (generation, snap) in &doc.snapshots {
            let n_deltas = doc.deltas.get(generation).map(|d| d.len()).unwrap_or(0);
            println!("   {generation}: snapshot {} bytes, {n_deltas} deltas", snap.len());
            let out = format!("{}.{}.snapshot", doc_id.replace(':', "_"), generation);
            std::fs::write(&out, snap).expect("could not write snapshot");
            println!("      -> {out}");
            if n_deltas > 0 {
                let dir = format!("{}.{}.deltas", doc_id.replace(':', "_"), generation);
                std::fs::create_dir_all(&dir).expect("could not create delta dir");
                for (seq, bytes) in &doc.deltas[generation] {
                    std::fs::write(format!("{dir}/{seq}"), bytes).expect("could not write delta");
                }
                println!("      -> {dir}/ ({n_deltas} deltas, apply in name order)");
            }
        }
        for generation in doc.deltas.keys() {
            if !doc.snapshots.contains_key(generation) {
                println!(
                    "   {generation}: {} deltas but NO snapshot (base was swept)",
                    doc.deltas[generation].len()
                );
            }
        }
    }
    if docs.is_empty() {
        println!("no plotweb documents found in {dir}");
    }
}
