//! Body round-trip (`chapter:` / `note:`) — the editor-collab CRDT seam.
//!
//! A chapter/note body is stored (in git, today) as `rinch-editor-core` `DocNode`
//! JSON. Migrating it means projecting that model onto a `rinch-editor-collab`
//! Automerge CRDT and materializing it back. [`roundtrip_body`] drives exactly that
//! path and asserts the result equals the input — flagging (never panicking) on any
//! step that can't project faithfully.

use std::rc::Rc;

use rinch_editor_collab::CollabSession;
use rinch_editor_core::serialize::{DocNode, slice_from_html};
use rinch_editor_core::{EditorState, Node, Schema, default_plugins};

use crate::RoundTrip;

/// Which body flavor is being round-tripped — decides how *legacy* (pre-DocNode)
/// content is converted to a model before the CRDT round-trip. New content
/// (DocNode JSON) takes the same path regardless of kind.
///
/// The conversion mirrors the editor's own legacy load (`editor_utils.rs`):
/// - [`Chapter`](BodyKind::Chapter) bodies were stored as **Markdown** →
///   `plotweb_common::markdown_to_html` → `load_html`.
/// - [`Note`](BodyKind::Note) bodies were stored as **raw HTML** → `load_html`
///   directly (empty → `<p></p>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// A chapter body (legacy form: Markdown).
    Chapter,
    /// A note body (legacy form: HTML).
    Note,
}

/// Round-trip one body document (`chapter:` or `note:`).
///
/// `content` is the raw stored content string. Real books mix two shapes and this
/// validator is tolerant of both, exactly as the editor is when it loads a body:
///
/// - **New content** — the editor's durable `DocNode` JSON save shape. Parsed with
///   `serde_json` and validated with `Schema::node_from_doc`.
/// - **Legacy content** — pre-DocNode bodies. A chapter is **Markdown**; a note is
///   **raw HTML**. These are converted to a model the *same way the editor does*
///   (`editor_utils.rs`): build HTML (`plotweb_common::markdown_to_html` for a
///   chapter, the HTML verbatim for a note — empty → `<p></p>`), then
///   `slice_from_html(&schema, html)` + `schema.branch("doc", slice.content)` — a
///   byte-for-byte mirror of `EditorHandle::load_html`.
///
/// Either way we obtain a model [`Node`], then drive the single CRDT path:
///
/// ```text
/// Node
///   → EditorState::create
///   → CollabSession::new               (project the model onto an Automerge CRDT)
///   → session.projected_doc(&schema)   (materialize the CRDT back to a model Node)
///   → Node::to_doc                     (serialize to the durable DocNode shape)
/// ```
///
/// The input is canonicalized through the *same* model (`Node::to_doc`) so the
/// comparison is semantic — JSON key/attr order and mark order are normalized by the
/// schema on both sides (see the crate-level equality note). For legacy content the
/// canonical form is the *converted* Node, so the audit reports whether that
/// converted body round-trips — the same body the editor would have saved.
///
/// Flags (with a specific, origin-tagged reason) on:
/// - malformed `DocNode` JSON that starts like JSON but fails schema validation,
/// - legacy HTML that `slice_from_html` cannot parse,
/// - `CollabSession::new` returning `Unsupported` (blockquote / table / image /
///   task-list / hard_break …) — the specific block type is captured in the reason.
///   A legacy chapter with a `> blockquote` converts fine but the collab projection
///   rejects `blockquote`, so it is correctly `Flagged` here,
/// - a materialized-≠-canonical mismatch (a high-level description of what differs).
///
/// Empty (or whitespace-only) content is [`RoundTrip::Clean`] — there is nothing to
/// migrate and an empty body projects trivially.
pub fn roundtrip_body(content: &str, kind: BodyKind) -> RoundTrip {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return RoundTrip::Clean;
    }

    // Share the ONE projection front-end with the backfill: obtain the exact same
    // hard-break-split model Node, then round-trip/compare on top of it. A flag from
    // the shared front-end becomes a `Flagged` here (unchanged behavior).
    let (schema, node, origin) = match prepare_body_node(content, kind) {
        Ok(prepared) => prepared,
        Err(reason) => return RoundTrip::flag(reason),
    };

    roundtrip_node(&schema, node, origin)
}

/// Project one body to its canonical Automerge **snapshot bytes** — the migration
/// backfill's emit endpoint, the exact bytes [`roundtrip_body`]'s validation is
/// computed over (they call the same [`prepare_body_node`] +
/// [`project_node_to_snapshot`], so a backfilled blob is byte-for-byte what the audit
/// certified clean).
///
/// Returns `Err(reason)` for a flagged body — the *same* reason strings
/// [`roundtrip_body`] produces (a malformed DocNode, unparseable legacy HTML, or an
/// unsupported block the collab projection rejects). A flagged body must never yield a
/// blob, so the caller writes nothing and leaves it on git.
///
/// An empty (or whitespace-only) body is Clean and migratable: it projects to the
/// snapshot of an empty doc (a single empty paragraph — what an empty editor holds).
pub fn project_body(content: &str, kind: BodyKind) -> Result<Vec<u8>, String> {
    let (schema, node, origin) = prepare_body_node(content, kind)?;
    project_node_to_snapshot(&schema, node, origin)
}

/// The single projection front-end shared by validate ([`roundtrip_body`]) and emit
/// ([`project_body`]): obtain the model [`Node`] (new DocNode JSON, or legacy
/// Markdown/HTML converted the editor's way), then apply the `hard_break` block-split
/// (Option 3). Returns the schema, the split node, and a human origin label for flag
/// reasons — or `Err(reason)` if the body cannot project faithfully.
///
/// Empty/whitespace content is a valid, migratable doc here: a single empty paragraph.
/// (`roundtrip_body` short-circuits empty to `Clean` before calling this; `project_body`
/// relies on this branch to emit the empty-doc snapshot.)
fn prepare_body_node(content: &str, kind: BodyKind) -> Result<(Rc<Schema>, Node, &'static str), String> {
    let schema = Rc::new(Schema::starter_kit());

    if content.trim().is_empty() {
        // An empty body projects trivially to a single empty paragraph — what the
        // editor holds for an empty note (`<p></p>`) — so an empty body is a Clean,
        // migratable doc rather than a hole in the store.
        let doc: DocNode =
            serde_json::from_str(r#"{"type":"doc","content":[{"type":"paragraph"}]}"#)
                .map_err(|e| format!("empty-body template did not parse: {e}"))?;
        let node = schema
            .node_from_doc(&doc)
            .map_err(|e| format!("empty-body template rejected by schema: {e}"))?;
        return Ok((schema, node, "empty body"));
    }

    // Obtain the model `Node` + a human origin label for reasons. New content is
    // DocNode JSON; anything else is legacy and is converted exactly as the editor
    // converts it on load.
    let (node, origin) = match parse_docnode(content.trim()) {
        // New content: DocNode JSON that parses. A schema rejection here is a genuine
        // broken-DocNode flag (not a fall-through to legacy).
        Some(doc) => match schema.node_from_doc(&doc) {
            Ok(n) => (n, "DocNode JSON"),
            Err(e) => {
                return Err(format!("DocNode JSON rejected by schema: {e}"));
            }
        },
        // Legacy content: build HTML the editor's way, then mirror `load_html`.
        None => {
            let (html, origin) = match kind {
                BodyKind::Chapter => (plotweb_common::markdown_to_html(content), "legacy markdown"),
                // Notes are already HTML; empty → a single empty paragraph, matching
                // `load_note_content`. (Whitespace-only already returned above.)
                BodyKind::Note => {
                    let html = if content.trim().is_empty() {
                        "<p></p>".to_string()
                    } else {
                        content.to_string()
                    };
                    (html, "legacy html")
                }
            };
            // Mirror `EditorHandle::load_html`: slice_from_html + schema.branch("doc", …).
            let slice = match slice_from_html(&schema, &html) {
                Ok(s) => s,
                Err(e) => {
                    return Err(format!("{origin} did not parse as HTML: {e}"));
                }
            };
            match schema.branch("doc", slice.content.clone()) {
                Ok(n) => (n, origin),
                Err(e) => {
                    return Err(format!("{origin} did not build a doc node: {e}"));
                }
            }
        }
    };

    // Option 3 (interim): the collab projection can't represent inline atoms yet
    // (`hard_break` / `image` / `horizontal_rule`). Split text-blocks at `hard_break`
    // into consecutive blocks — a legacy note's `<br>` becomes a paragraph break — so
    // those notes migrate instead of flagging. No inline content is dropped; only the
    // break atom becomes a block boundary. It is a no-op for content without breaks,
    // and it is part of the canonical migration form, so the backfill stores exactly
    // what the audit validates here. (`image`/`horizontal_rule` atoms still flag —
    // dropping them would be lossy — until the projection supports them.)
    let doc = node
        .to_doc()
        .map_err(|e| format!("could not read body to split breaks ({origin}): {e}"))?;
    let split = split_hard_breaks_doc(&doc);
    let node = schema
        .node_from_doc(&split)
        .map_err(|e| format!("hard_break split produced an invalid doc ({origin}): {e}"))?;

    Ok((schema, node, origin))
}

/// Project a prepared model `node` onto the editor-collab Automerge CRDT and return
/// its snapshot bytes — the single point where the CRDT is built, shared by validate
/// and emit. `Err(reason)` when the collab projection rejects an unsupported shape
/// (the block type is captured in the reason).
fn project_node_to_snapshot(schema: &Rc<Schema>, node: Node, origin: &str) -> Result<Vec<u8>, String> {
    let state = EditorState::create(schema.clone(), node, default_plugins());
    let mut session = CollabSession::new(&state)
        .map_err(|e| format!("editor-collab cannot project this body ({origin}): {e}"))?;
    Ok(session.snapshot())
}

/// Container block types whose children are themselves blocks — recursed into so a
/// `hard_break` inside e.g. a list item's paragraph is still split.
fn is_container_type(t: &str) -> bool {
    matches!(
        t,
        "bullet_list" | "ordered_list" | "list_item" | "blockquote" | "table" | "table_row"
            | "table_cell"
    )
}

/// Split every text-block that contains inline `hard_break` nodes into consecutive
/// blocks of the same type, dropping the breaks; recurse into container blocks. No
/// inline content is dropped — only `hard_break` atoms become block boundaries — so
/// a legacy `<br>` reads as a paragraph break after migration (Option 3). A no-op for
/// content with no breaks.
fn split_hard_breaks_doc(node: &DocNode) -> DocNode {
    let mut out: Vec<DocNode> = Vec::with_capacity(node.content.len());
    for child in &node.content {
        let has_break = child.content.iter().any(|g| g.node_type == "hard_break");
        if has_break {
            // Partition the inline content at each `hard_break`.
            let mut segments: Vec<Vec<DocNode>> = vec![Vec::new()];
            for inline in &child.content {
                if inline.node_type == "hard_break" {
                    segments.push(Vec::new());
                } else {
                    segments.last_mut().unwrap().push(inline.clone());
                }
            }
            let mk_block = |content: Vec<DocNode>| DocNode {
                node_type: child.node_type.clone(),
                attrs: child.attrs.clone(),
                content,
                text: child.text.clone(),
                marks: child.marks.clone(),
            };
            let mut emitted = 0usize;
            for seg in segments {
                if seg.is_empty() {
                    continue; // drop empty segments (leading/trailing/consecutive breaks)
                }
                emitted += 1;
                out.push(mk_block(seg));
            }
            if emitted == 0 {
                // The block held only break(s): preserve one empty block of its type.
                out.push(mk_block(Vec::new()));
            }
        } else if is_container_type(&child.node_type) {
            out.push(split_hard_breaks_doc(child));
        } else {
            out.push(child.clone());
        }
    }
    DocNode {
        node_type: node.node_type.clone(),
        attrs: node.attrs.clone(),
        content: out,
        text: node.text.clone(),
        marks: node.marks.clone(),
    }
}

/// Parse `content` as `DocNode` JSON, mirroring the editor's `load_docnode`: only
/// content that trims to a leading `{` and deserializes cleanly is treated as new
/// content; everything else (`None`) is legacy and converted downstream.
fn parse_docnode(content: &str) -> Option<DocNode> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }
    serde_json::from_str::<DocNode>(trimmed).ok()
}

/// The shared CRDT round-trip: canonicalize `node`, project it onto an Automerge
/// CRDT, materialize it back, and compare. `origin` labels the flag reasons so the
/// audit shows whether a flagged body came from DocNode JSON or converted legacy
/// content.
fn roundtrip_node(schema: &Rc<Schema>, node: Node, origin: &str) -> RoundTrip {
    // Canonical form of the ORIGINAL: same model → same serializer as the round-trip
    // endpoint, so attr/mark/key ordering is normalized identically on both sides.
    let canonical = match node.to_doc() {
        Ok(d) => d,
        Err(e) => return RoundTrip::flag(format!("could not canonicalize the original: {e}")),
    };

    // 3. Project onto the CRDT — via the SAME endpoint the backfill emits from, so the
    //    bytes validated here are byte-for-byte the bytes a blob would hold. Unsupported
    //    shapes fail loud (flagged, never a blob).
    let bytes = match project_node_to_snapshot(schema, node, origin) {
        Ok(b) => b,
        Err(reason) => return RoundTrip::flag(reason),
    };

    // 4. Materialize the CRDT back through the model to the durable shape. A save/load
    //    round-trip of the Automerge bytes is exercised so we validate the *durable*
    //    projection, not just the in-memory one.
    let loaded = match CollabSession::from_bytes(&bytes) {
        Ok(s) => s,
        Err(e) => return RoundTrip::flag(format!("saved CRDT bytes did not reload: {e}")),
    };
    let projected = match loaded.projected_doc(schema) {
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
        RoundTrip::flag(format!(
            "materialized body differs from original ({origin}): {detail}"
        ))
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
        assert_eq!(roundtrip_body(json, BodyKind::Chapter), RoundTrip::Clean, "clean prose must be Clean");
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
            roundtrip_body(&json, BodyKind::Chapter),
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
            roundtrip_body(json, BodyKind::Chapter),
            RoundTrip::Clean,
            "over-segmented same-mark runs coalesce losslessly"
        );
    }

    /// Empty content: nothing to migrate → Clean.
    #[test]
    fn empty_is_clean() {
        assert_eq!(roundtrip_body("", BodyKind::Chapter), RoundTrip::Clean);
        assert_eq!(roundtrip_body("   \n  ", BodyKind::Chapter), RoundTrip::Clean);
        assert_eq!(roundtrip_body("", BodyKind::Note), RoundTrip::Clean);
        assert_eq!(roundtrip_body("   \n  ", BodyKind::Note), RoundTrip::Clean);
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
        let rt = roundtrip_body(json, BodyKind::Chapter);
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
        let rt = roundtrip_body(json, BodyKind::Chapter);
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
        let rt = roundtrip_body(json, BodyKind::Chapter);
        let reason = rt.reason().expect("image must flag");
        assert!(
            reason.contains("image") || reason.contains("inline"),
            "reason should mention the unsupported atom, got: {reason}"
        );
    }

    // ── Legacy (pre-DocNode) content ────────────────────────────────────────
    //
    // ~Half of real content predates the DocNode migration: chapters are Markdown,
    // notes are HTML. The audit must convert these the SAME way the editor does
    // (`plotweb_common::markdown_to_html` + `load_html`) and then round-trip, not
    // flag them wholesale as "not DocNode JSON".

    /// The fidelity guarantee for real books: a legacy chapter with several sentences
    /// on their own lines (no blank lines) must NOT collapse into one paragraph. The
    /// line-based converter yields one paragraph per line, and that survives the
    /// round-trip → Clean, with one paragraph per source line in the materialized doc.
    #[test]
    fn legacy_markdown_one_paragraph_per_line_is_clean() {
        let md = "The lantern guttered against the fog.\n\
                  Kal plucked another quill off the corpse.\n\
                  \"I'm used to you talking crazy,\" he said.\n\
                  For once, I agree.";

        // Prove the converter + HTML parse gives one paragraph per line (no collapse).
        // This is exactly the Node roundtrip_body builds for a legacy chapter.
        let schema = Schema::starter_kit();
        let html = plotweb_common::markdown_to_html(md);
        let slice = slice_from_html(&schema, &html).expect("converted legacy md must parse");
        let doc = schema
            .branch("doc", slice.content.clone())
            .expect("slice must build a doc");
        let docnode = doc.to_doc().expect("doc serializes");
        assert_eq!(
            docnode.content.len(),
            4,
            "one paragraph per source line (no collapse), got {} blocks",
            docnode.content.len()
        );
        for block in &docnode.content {
            assert_eq!(
                block.node_type, "paragraph",
                "every line becomes a paragraph, got `{}`",
                block.node_type
            );
        }

        // …and it round-trips through the CRDT losslessly.
        assert_eq!(
            roundtrip_body(md, BodyKind::Chapter),
            RoundTrip::Clean,
            "line-per-paragraph legacy markdown must be Clean"
        );
    }

    /// A legacy chapter using heading + inline marks + a bullet list — all within the
    /// collab scope — converts and round-trips Clean, structure preserved.
    #[test]
    fn legacy_markdown_heading_marks_list_is_clean() {
        let md = "# Chapter One\n\
                  Some **bold** and *italic* prose.\n\
                  - first item\n\
                  - second item";

        // Structure check: heading, paragraph, bullet_list.
        let schema = Schema::starter_kit();
        let html = plotweb_common::markdown_to_html(md);
        let slice = slice_from_html(&schema, &html).expect("converted legacy md must parse");
        let doc = schema.branch("doc", slice.content.clone()).unwrap();
        let docnode = doc.to_doc().unwrap();
        let kinds: Vec<&str> = docnode.content.iter().map(|n| n.node_type.as_str()).collect();
        assert_eq!(
            kinds,
            vec!["heading", "paragraph", "bullet_list"],
            "legacy markdown structure must be preserved"
        );

        assert_eq!(
            roundtrip_body(md, BodyKind::Chapter),
            RoundTrip::Clean,
            "heading/marks/list legacy markdown must be Clean"
        );
    }

    /// A legacy note is raw HTML (already), not markdown. It converts via the note
    /// path (HTML verbatim → load_html) and round-trips Clean.
    #[test]
    fn legacy_html_note_is_clean() {
        let html = "<p>The lantern <strong>guttered</strong> against the fog.</p>";
        assert_eq!(
            roundtrip_body(html, BodyKind::Note),
            RoundTrip::Clean,
            "legacy HTML note must be Clean"
        );
    }

    /// Option 3 (interim): a legacy HTML note with `<br>` (which becomes a
    /// `hard_break` inline atom the collab projection can't represent yet) is split at
    /// the break into paragraphs and migrates Clean, dropping no text.
    #[test]
    fn legacy_html_note_with_hard_break_is_clean() {
        let html = "<p>First line.<br>Second line.<br>Third line.</p>";

        // Structural: the converted note has one paragraph with a hard_break; the
        // split turns it into three paragraphs and leaves no hard_break behind.
        let schema = Schema::starter_kit();
        let slice = slice_from_html(&schema, html).unwrap();
        let doc = schema.branch("doc", slice.content.clone()).unwrap();
        let before = doc.to_doc().unwrap();
        let after = split_hard_breaks_doc(&before);
        assert_eq!(
            after.content.len(),
            3,
            "two <br> split one paragraph into three, got {} blocks",
            after.content.len()
        );
        assert!(
            after.content.iter().all(|b| b.node_type == "paragraph"),
            "split blocks keep the paragraph type"
        );
        fn has_hb(n: &DocNode) -> bool {
            n.node_type == "hard_break" || n.content.iter().any(has_hb)
        }
        assert!(!has_hb(&after), "no hard_break remains after the split");

        // …and end-to-end it round-trips Clean (was flagged before Option 3).
        assert_eq!(
            roundtrip_body(html, BodyKind::Note),
            RoundTrip::Clean,
            "a note with <br> must split and be Clean"
        );
    }

    /// A legacy chapter containing a `> blockquote` converts fine (markdown_to_html
    /// emits `<blockquote>`), but blockquote is outside the collab projection, so it
    /// is correctly Flagged — with the block type named, and left on git.
    #[test]
    fn legacy_markdown_blockquote_is_flagged() {
        let md = "A line before.\n> a quoted line\nA line after.";
        let rt = roundtrip_body(md, BodyKind::Chapter);
        let reason = rt.reason().expect("legacy blockquote must flag");
        assert!(
            reason.contains("blockquote"),
            "reason should name the unsupported block type, got: {reason}"
        );
    }

    // ── project_body: the backfill emit endpoint ────────────────────────────────

    /// `project_body` and `roundtrip_body` must agree on Clean vs Flagged for every
    /// shape — they share the one projection front-end, so a body the audit certified
    /// Clean yields blob bytes, and a flagged one yields the same-worded `Err`.
    #[test]
    fn project_body_agrees_with_roundtrip() {
        let clean = r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"hello"}]}
        ]}"#;
        let flagged = r#"{"type":"doc","content":[
            {"type":"blockquote","content":[
                {"type":"paragraph","content":[{"type":"text","text":"quoted"}]}
            ]}
        ]}"#;

        // Clean → roundtrip Clean AND project Ok(non-empty bytes).
        assert_eq!(roundtrip_body(clean, BodyKind::Chapter), RoundTrip::Clean);
        let bytes = project_body(clean, BodyKind::Chapter).expect("clean body projects");
        assert!(!bytes.is_empty(), "a clean projection must have bytes");

        // Flagged → roundtrip Flagged AND project Err with the same reason.
        let rt = roundtrip_body(flagged, BodyKind::Chapter);
        let err = project_body(flagged, BodyKind::Chapter).expect_err("flagged body has no blob");
        assert_eq!(rt.reason().unwrap(), err, "flag reason must match exactly");
    }

    /// An empty body projects to a real, loadable Automerge snapshot (a single empty
    /// paragraph) — an empty body is Clean and migratable, not a hole in the store.
    #[test]
    fn project_body_empty_is_loadable() {
        for kind in [BodyKind::Chapter, BodyKind::Note] {
            assert_eq!(roundtrip_body("", kind), RoundTrip::Clean);
            let bytes = project_body("", kind).expect("empty body projects");
            assert!(!bytes.is_empty(), "empty-doc snapshot has bytes");
            // It reloads as a real CRDT and materializes to a one-block doc.
            let session = CollabSession::from_bytes(&bytes).expect("empty snapshot reloads");
            let schema = Rc::new(Schema::starter_kit());
            let doc = session
                .projected_doc(&schema)
                .expect("materializes")
                .to_doc()
                .expect("serializes");
            assert_eq!(doc.node_type, "doc");
            assert_eq!(doc.content.len(), 1, "empty body is a single empty block");
        }
    }

    /// A projected blob loads back as a real Automerge doc and materializes to the
    /// expected DocNode — proof the emitted bytes are a genuine, loadable CRDT, not
    /// garbage. (The same guarantee the backfill relies on.)
    #[test]
    fn project_body_blob_materializes_to_expected_docnode() {
        let json = r#"{"type":"doc","content":[
            {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Title"}]},
            {"type":"paragraph","content":[{"type":"text","text":"Body text."}]}
        ]}"#;
        let bytes = project_body(json, BodyKind::Chapter).expect("projects");

        let schema = Rc::new(Schema::starter_kit());
        let session = CollabSession::from_bytes(&bytes).expect("blob reloads");
        let materialized = session
            .projected_doc(&schema)
            .expect("materializes")
            .to_doc()
            .expect("serializes");

        // Canonicalize the source through the same model for a fair compare.
        let src: DocNode = serde_json::from_str(json).unwrap();
        let canonical = schema.node_from_doc(&src).unwrap().to_doc().unwrap();
        assert_eq!(
            coalesce(&materialized),
            coalesce(&canonical),
            "the blob must materialize to the source DocNode"
        );
    }
}
