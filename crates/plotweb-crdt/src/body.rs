//! Body round-trip (`chapter:` / `note:`) — the editor-collab CRDT seam.
//!
//! A chapter/note body is stored (in git, today) as `rinch-editor-core` `DocNode`
//! JSON. Migrating it means projecting that model onto a `rinch-editor-collab`
//! Automerge CRDT and materializing it back. [`roundtrip_body`] drives exactly that
//! path and asserts the result equals the input — flagging (never panicking) on any
//! step that can't project faithfully.

use std::rc::Rc;

use rinch_editor_collab::CollabSession;
use rinch_editor_core::serialize::DocNode;
use rinch_editor_core::{EditorState, Schema, default_plugins};

use crate::RoundTrip;

/// Round-trip one body document (`chapter:` or `note:`).
///
/// `content_docnode_json` is the raw stored content string (the editor's durable
/// `DocNode` JSON save shape). The path mirrors the client body primitive:
///
/// ```text
/// DocNode JSON
///   → Schema::node_from_doc            (parse + schema-validate)
///   → EditorState::create
///   → CollabSession::new               (project the model onto an Automerge CRDT)
///   → session.projected_doc(&schema)   (materialize the CRDT back to a model Node)
///   → Node::to_doc                     (serialize to the durable DocNode shape)
/// ```
///
/// The input is canonicalized through the *same* model (`node_from_doc → to_doc`) so
/// the comparison is semantic — JSON key/attr order and mark order are normalized by
/// the schema on both sides (see the crate-level equality note).
///
/// Flags (with a specific reason) on:
/// - not valid `DocNode` JSON,
/// - `node_from_doc` schema-validation failure,
/// - `CollabSession::new` returning `Unsupported` (blockquote / table / image /
///   task-list / hard_break …) — the specific block type is captured in the reason,
/// - a materialized-≠-canonical mismatch (a high-level description of what differs).
///
/// Empty (or whitespace-only) content is [`RoundTrip::Clean`] — there is nothing to
/// migrate and an empty body projects trivially.
pub fn roundtrip_body(content_docnode_json: &str) -> RoundTrip {
    let trimmed = content_docnode_json.trim();
    if trimmed.is_empty() {
        return RoundTrip::Clean;
    }

    let schema = Rc::new(Schema::starter_kit());

    // 1. Parse the durable wire shape.
    let doc: DocNode = match serde_json::from_str(trimmed) {
        Ok(d) => d,
        Err(e) => return RoundTrip::flag(format!("not valid DocNode JSON: {e}")),
    };

    // 2. Into the schema-validated model.
    let node = match schema.node_from_doc(&doc) {
        Ok(n) => n,
        Err(e) => return RoundTrip::flag(format!("node_from_doc rejected the content: {e}")),
    };

    // Canonical form of the ORIGINAL: same model → same serializer as the round-trip
    // endpoint, so attr/mark/key ordering is normalized identically on both sides.
    let canonical = match node.to_doc() {
        Ok(d) => d,
        Err(e) => return RoundTrip::flag(format!("could not canonicalize the original: {e}")),
    };

    // 3. Project onto the CRDT. This is where unsupported shapes fail loud.
    let state = EditorState::create(schema.clone(), node, default_plugins());
    let mut session = match CollabSession::new(&state) {
        Ok(s) => s,
        Err(e) => return RoundTrip::flag(format!("editor-collab cannot project this body: {e}")),
    };

    // 4. Materialize the CRDT back through the model to the durable shape. A save/load
    //    round-trip of the Automerge bytes is exercised so we validate the *durable*
    //    projection, not just the in-memory one.
    let bytes = session.snapshot();
    let loaded = match CollabSession::from_bytes(&bytes) {
        Ok(s) => s,
        Err(e) => return RoundTrip::flag(format!("saved CRDT bytes did not reload: {e}")),
    };
    let projected = match loaded.projected_doc(&schema) {
        Ok(n) => n,
        Err(e) => return RoundTrip::flag(format!("could not materialize the CRDT: {e}")),
    };
    let materialized = match projected.to_doc() {
        Ok(d) => d,
        Err(e) => return RoundTrip::flag(format!("could not serialize the materialized doc: {e}")),
    };

    // 5. Semantic comparison of the two canonical DocNodes.
    //
    // Both sides are first *coalesced* (adjacent same-mark inline text runs merged,
    // empty text runs dropped) so that a difference in inline *segmentation* is not
    // mistaken for a difference in *content*. This matters for real data: the
    // markdown importer emits a paragraph as many separately-segmented text nodes
    // (split at sentence boundaries), while the CRDT stores per-block text as one
    // `Text` and materializes it back as one span per mark boundary. Same characters,
    // same per-character marks, different node count — semantically identical.
    // Comparing the coalesced forms is the correct "structure + text + marks" equality
    // (a byte/count comparison here would be a false Flagged).
    let canonical = coalesce(&canonical);
    let materialized = coalesce(&materialized);
    if materialized == canonical {
        RoundTrip::Clean
    } else {
        let detail = first_diff(&canonical, &materialized, "doc")
            .unwrap_or_else(|| "content differs (no single node located)".to_string());
        RoundTrip::flag(format!("materialized body differs from original: {detail}"))
    }
}

/// Is `n` an inline text node (carries a text string)?
fn is_text(n: &DocNode) -> bool {
    n.node_type == "text" && n.text.is_some()
}

/// Canonicalize inline segmentation: recursively merge consecutive text-node children
/// that carry identical marks (and attrs) into one, concatenating their text, and drop
/// empty text runs. Two inline sequences with the same per-character text + marks
/// coalesce to the same form regardless of how the source split them into nodes.
fn coalesce(node: &DocNode) -> DocNode {
    let mut content: Vec<DocNode> = Vec::with_capacity(node.content.len());
    for child in &node.content {
        let child = coalesce(child);
        // Drop empty text runs — they contribute no characters.
        if is_text(&child) && child.text.as_deref() == Some("") {
            continue;
        }
        if let Some(last) = content.last_mut()
            && is_text(last)
            && is_text(&child)
            && last.marks == child.marks
            && last.attrs == child.attrs
        {
            let mut text = last.text.take().unwrap_or_default();
            text.push_str(child.text.as_deref().unwrap_or_default());
            last.text = Some(text);
            continue;
        }
        content.push(child);
    }
    DocNode {
        node_type: node.node_type.clone(),
        attrs: node.attrs.clone(),
        content,
        text: node.text.clone(),
        marks: node.marks.clone(),
    }
}

/// Locate the first meaningful difference between two `DocNode`s and describe it at a
/// high level (node type / attrs / text / marks / child count, with a positional
/// path). Used only to explain a flag — the Clean/Flagged decision is the full
/// `DocNode` equality above, not this walk.
fn first_diff(a: &DocNode, b: &DocNode, path: &str) -> Option<String> {
    if a.node_type != b.node_type {
        return Some(format!(
            "{path}: node type `{}` became `{}`",
            a.node_type, b.node_type
        ));
    }
    if a.attrs != b.attrs {
        return Some(format!(
            "{path} (`{}`): attrs differ ({:?} vs {:?})",
            a.node_type, a.attrs, b.attrs
        ));
    }
    if a.text != b.text {
        return Some(format!(
            "{path} (`{}`): text differs ({:?} vs {:?})",
            a.node_type, a.text, b.text
        ));
    }
    if a.marks != b.marks {
        return Some(format!(
            "{path} (`{}`): marks differ ({:?} vs {:?})",
            a.node_type, a.marks, b.marks
        ));
    }
    if a.content.len() != b.content.len() {
        return Some(format!(
            "{path} (`{}`): child count {} vs {}",
            a.node_type,
            a.content.len(),
            b.content.len()
        ));
    }
    for (i, (ca, cb)) in a.content.iter().zip(b.content.iter()).enumerate() {
        if let Some(d) = first_diff(ca, cb, &format!("{path}/{i}")) {
            return Some(d);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clean prose: paragraphs, headings, marks (bold/italic/code), and both list
    /// kinds — all inside the staged collab scope, so it must round-trip losslessly.
    #[test]
    fn clean_prose_is_clean() {
        let json = r#"{"type":"doc","content":[
            {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Chapter One"}]},
            {"type":"paragraph","content":[
                {"type":"text","text":"The lantern "},
                {"type":"text","text":"guttered","marks":[{"type":"bold"}]},
                {"type":"text","text":" against the "},
                {"type":"text","text":"fog","marks":[{"type":"italic"}]},
                {"type":"text","text":"."}
            ]},
            {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"A subhead"}]},
            {"type":"paragraph","content":[
                {"type":"text","text":"Inline "},
                {"type":"text","text":"code()","marks":[{"type":"code"}]},
                {"type":"text","text":" here."}
            ]},
            {"type":"bullet_list","content":[
                {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},
                {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}
            ]},
            {"type":"ordered_list","attrs":{"start":1},"content":[
                {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},
                {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}
            ]}
        ]}"#;
        assert_eq!(roundtrip_body(json), RoundTrip::Clean, "clean prose must be Clean");
    }

    /// The pathological single giant paragraph: one block, a huge text run. Ugly but
    /// perfectly flat — must be Clean (lossless).
    #[test]
    fn single_giant_paragraph_is_clean() {
        let big = "word ".repeat(20_000);
        let json = format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":{}}}]}}]}}"#,
            serde_json::to_string(&big).unwrap()
        );
        assert_eq!(
            roundtrip_body(&json),
            RoundTrip::Clean,
            "a single giant paragraph is ugly but lossless"
        );
    }

    /// Over-segmented inline runs (the real markdown-importer shape): one paragraph
    /// whose text is split into many separate same-mark text nodes plus interspersed
    /// italic spans. The CRDT coalesces adjacent same-mark runs; the content is
    /// identical, so it must be Clean (segmentation is not a content difference).
    #[test]
    fn over_segmented_paragraph_is_clean() {
        let json = r#"{"type":"doc","content":[{"type":"paragraph","content":[
            {"type":"text","text":"Kal plucked another quill"},
            {"type":"text","text":" off the corpse. "},
            {"type":"text","text":"\"I'm used to you talking crazy,\""},
            {"type":"text","text":" he said. "},
            {"type":"text","text":"felt","marks":[{"type":"italic"}]},
            {"type":"text","text":" like it had been a full day. "},
            {"type":"text","text":"For once","marks":[{"type":"italic"}]},
            {"type":"text","text":", I agree."}
        ]}]}"#;
        assert_eq!(
            roundtrip_body(json),
            RoundTrip::Clean,
            "over-segmented same-mark runs coalesce losslessly"
        );
    }

    /// Empty content: nothing to migrate → Clean.
    #[test]
    fn empty_is_clean() {
        assert_eq!(roundtrip_body(""), RoundTrip::Clean);
        assert_eq!(roundtrip_body("   \n  "), RoundTrip::Clean);
    }

    /// A blockquote is outside the staged collab scope → Flagged, with the block type
    /// named, and NOT corrupted (it stays on git).
    #[test]
    fn blockquote_is_flagged_with_type() {
        let json = r#"{"type":"doc","content":[
            {"type":"blockquote","content":[
                {"type":"paragraph","content":[{"type":"text","text":"quoted"}]}
            ]}
        ]}"#;
        let rt = roundtrip_body(json);
        let reason = rt.reason().expect("blockquote must flag");
        assert!(
            reason.contains("blockquote"),
            "reason should name the block type, got: {reason}"
        );
    }

    /// A table is unsupported → Flagged naming the block type.
    #[test]
    fn table_is_flagged_with_type() {
        let json = r#"{"type":"doc","content":[
            {"type":"table","content":[
                {"type":"table_row","content":[
                    {"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]}
                ]}
            ]}
        ]}"#;
        let rt = roundtrip_body(json);
        let reason = rt.reason().expect("table must flag");
        assert!(
            reason.contains("table"),
            "reason should name the block type, got: {reason}"
        );
    }

    /// An inline image atom is unsupported → Flagged.
    #[test]
    fn image_is_flagged() {
        let json = r#"{"type":"doc","content":[
            {"type":"paragraph","content":[
                {"type":"text","text":"before "},
                {"type":"image","attrs":{"src":"hash://abc","alt":""}},
                {"type":"text","text":" after"}
            ]}
        ]}"#;
        let rt = roundtrip_body(json);
        let reason = rt.reason().expect("image must flag");
        assert!(
            reason.contains("image") || reason.contains("inline"),
            "reason should mention the unsupported atom, got: {reason}"
        );
    }

    /// Not JSON at all (e.g. legacy raw markdown left in a body) → Flagged, not panic.
    #[test]
    fn non_json_is_flagged() {
        let rt = roundtrip_body("# Not JSON\n\nJust markdown.");
        assert!(rt.reason().is_some(), "non-JSON must flag, not panic");
    }
}
