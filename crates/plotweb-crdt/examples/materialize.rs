//! Recovery aid: materialize a stored body document (`chapter:`/`note:` snapshot
//! bytes, as served by `GET /api/books/{id}/sync/{doc_id}`) back to DocNode JSON,
//! and print its plain text so a lost draft can be eyeballed.
//!
//!     cargo run -p plotweb-crdt --example materialize -- <snapshot.bin> [--json]
//!
//! Read-only: it never writes to a store or a server.

fn text_of(v: &serde_json::Value, out: &mut String) {
    match v {
        serde_json::Value::Object(m) => {
            // Two shapes carry text: the DocNode form (a `text` node with a `text`
            // field) and the older Automerge projection (the block node itself holds
            // `text`). Taking any string `text` covers both without double-counting,
            // since a DocNode block never has one.
            if let Some(s) = m.get("text").and_then(|t| t.as_str()) {
                out.push_str(s);
            }
            if let Some(kids) = m.get("content").and_then(|c| c.as_array()) {
                for k in kids {
                    text_of(k, out);
                }
            }
            if matches!(
                m.get("type").and_then(|t| t.as_str()),
                Some("paragraph") | Some("heading")
            ) {
                out.push_str("\n\n");
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|k| text_of(k, out)),
        _ => {}
    }
}

/// Decode the bytes as a plain yrs update and re-encode the document as JSON.
fn raw_yrs_json(bytes: &[u8]) -> Result<String, String> {
    use yrs::types::ToJson;
    use yrs::updates::decoder::Decode;
    use yrs::{GetString, ReadTxn, Transact};

    let doc = yrs::Doc::new();
    let update = yrs::Update::decode_v1(bytes)
        .or_else(|_| yrs::Update::decode_v2(bytes))
        .map_err(|e| format!("not a yrs v1 or v2 update: {e}"))?;
    doc.transact_mut()
        .apply_update(update)
        .map_err(|e| format!("update did not apply: {e}"))?;

    // yrs only surfaces a root type that has been resolved locally, so name the two
    // the editor projection uses (`projection.rs`: root Array "content", root Map
    // "meta") before reading. Without this `root_refs()` comes back empty.
    let _content = doc.get_or_insert_array("content");
    let _meta = doc.get_or_insert_map("meta");

    let txn = doc.transact();
    let mut roots = serde_json::Map::new();
    for (name, value) in txn.root_refs() {
        // `Out` itself has no JSON conversion; each concrete ref does.
        let any = match value {
            yrs::Out::Any(a) => a,
            yrs::Out::YMap(m) => m.to_json(&txn),
            yrs::Out::YArray(a) => a.to_json(&txn),
            yrs::Out::YXmlFragment(f) => yrs::Any::from(f.get_string(&txn)),
            yrs::Out::YXmlElement(e) => yrs::Any::from(e.get_string(&txn)),
            yrs::Out::YText(t) => yrs::Any::from(t.get_string(&txn)),
            yrs::Out::YXmlText(t) => yrs::Any::from(t.get_string(&txn)),
            other => yrs::Any::from(other.to_string(&txn)),
        };
        let mut buf = String::new();
        any.to_json(&mut buf);
        let parsed: serde_json::Value =
            serde_json::from_str(&buf).map_err(|e| format!("root `{name}` did not re-parse: {e}"))?;
        roots.insert(name.to_string(), parsed);
    }
    serde_json::to_string(&serde_json::Value::Object(roots))
        .map_err(|e| format!("could not encode JSON: {e}"))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: materialize <snapshot.bin> [--json|--history <out_dir>]");
    let mode = args.next();
    let want_json = mode.as_deref() == Some("--json");
    let want_history = mode.as_deref() == Some("--history");
    let history_dir = args.next().unwrap_or_else(|| "recovered".to_string());

    let bytes = std::fs::read(&path).expect("could not read snapshot");

    if want_history {
        if let Err(e) = replay_history(&bytes, &history_dir) {
            eprintln!("could not replay {path}: {e}");
            std::process::exit(1);
        }
        return;
    }

    let json = match plotweb_crdt::materialize_body(&bytes) {
        Ok(j) => j,
        // Older projections (pre-`meta.format`) are rejected by the collab loader.
        // They are still ordinary yrs updates, so fall back to decoding the document
        // directly — enough to read the text out, which is all recovery needs.
        Err(e) => {
            eprintln!("[collab load failed: {e}]");
            eprintln!("[falling back to a raw yrs decode]");
            raw_yrs_json(&bytes)
                .or_else(|yrs_err| {
                    eprintln!("[yrs decode failed: {yrs_err}]");
                    eprintln!("[falling back to an Automerge decode]");
                    automerge_json(&bytes)
                })
                .unwrap_or_else(|e| {
                    eprintln!("could not decode {path}: {e}");
                    std::process::exit(1);
                })
        }
    };

    if want_json {
        println!("{json}");
        return;
    }

    let doc: serde_json::Value = serde_json::from_str(&json).expect("materialized JSON did not parse");
    let mut text = String::new();
    text_of(&doc, &mut text);
    eprintln!("[{} bytes of DocNode JSON, {} chars of text]", json.len(), text.chars().count());
    println!("{}", text.trim_end());
}

// ── Automerge fallback ───────────────────────────────────────────────────────
//
// Bodies projected before the yrs collab seam are Automerge documents (magic
// `85 6f 4a 83`). Automerge keeps every change, so these can be *replayed*: the
// state before an accidental deletion or overwrite is still in the file.

/// Convert an Automerge object to `serde_json`, recursively.
fn am_to_json<R: automerge::ReadDoc>(doc: &R, obj: &automerge::ObjId) -> serde_json::Value {
    use automerge::{ObjType, Value};
    match doc.object_type(obj) {
        Ok(ObjType::Text) => serde_json::Value::String(doc.text(obj).unwrap_or_default()),
        Ok(ObjType::List) => serde_json::Value::Array(
            (0..doc.length(obj))
                .map(|i| match doc.get(obj, i) {
                    Ok(Some((Value::Object(_), id))) => am_to_json(doc, &id),
                    Ok(Some((Value::Scalar(s), _))) => scalar_to_json(&s),
                    _ => serde_json::Value::Null,
                })
                .collect(),
        ),
        Ok(ObjType::Map) | Ok(ObjType::Table) => {
            let mut m = serde_json::Map::new();
            for key in doc.keys(obj) {
                let v = match doc.get(obj, &key) {
                    Ok(Some((Value::Object(_), id))) => am_to_json(doc, &id),
                    Ok(Some((Value::Scalar(s), _))) => scalar_to_json(&s),
                    _ => serde_json::Value::Null,
                };
                m.insert(key, v);
            }
            serde_json::Value::Object(m)
        }
        Err(_) => serde_json::Value::Null,
    }
}

fn scalar_to_json(s: &automerge::ScalarValue) -> serde_json::Value {
    use automerge::ScalarValue::*;
    match s {
        Str(v) => serde_json::Value::String(v.to_string()),
        Int(v) => (*v).into(),
        Uint(v) => (*v).into(),
        F64(v) => serde_json::json!(v),
        Boolean(v) => (*v).into(),
        Counter(v) => serde_json::json!(i64::from(v)),
        Timestamp(v) => (*v).into(),
        Bytes(b) => serde_json::Value::String(format!("<{} bytes>", b.len())),
        Null | Unknown { .. } => serde_json::Value::Null,
    }
}

fn automerge_json(bytes: &[u8]) -> Result<String, String> {
    let doc = automerge::Automerge::load(bytes).map_err(|e| format!("not an Automerge doc: {e}"))?;
    let value = am_to_json(&doc, &automerge::ROOT);
    serde_json::to_string(&value).map_err(|e| format!("could not encode JSON: {e}"))
}

/// Replay the document change by change, reporting the text at every step.
///
/// The point of the exercise: a draft that was deleted or overwritten still exists
/// at some earlier point in the change log, and the longest state is almost always
/// the one wanted back.
fn replay_history(bytes: &[u8], out_dir: &str) -> Result<(), String> {
    let full = automerge::Automerge::load(bytes).map_err(|e| format!("not an Automerge doc: {e}"))?;
    let changes: Vec<_> = full.get_changes(&[]).into_iter().cloned().collect();
    eprintln!("[{} changes in this document]", changes.len());
    std::fs::create_dir_all(out_dir).map_err(|e| format!("could not create {out_dir}: {e}"))?;

    let mut doc = automerge::Automerge::new();
    let mut best = (0usize, 0usize, String::new()); // (chars, step, text)
    let mut prev_len = 0usize;

    for (i, change) in changes.iter().enumerate() {
        doc.apply_changes([change.clone()])
            .map_err(|e| format!("change {i} did not apply: {e}"))?;
        let json = am_to_json(&doc, &automerge::ROOT);
        let mut text = String::new();
        text_of(&json, &mut text);
        let len = text.chars().count();

        // Report growth, and every drop — a drop is where content went missing.
        if len + 200 < prev_len {
            eprintln!("  step {i}: text SHRANK {prev_len} -> {len} chars  <-- content lost here");
        }
        if len > best.0 {
            best = (len, i, text.clone());
        }
        prev_len = len;
    }

    let path = format!("{out_dir}/longest-step{}.txt", best.1);
    std::fs::write(&path, best.2.trim_end()).map_err(|e| format!("could not write {path}: {e}"))?;
    eprintln!("[longest state: {} chars at change {} -> {}]", best.0, best.1, path);
    Ok(())
}
