//! Manuscript export — the mirror image of `plotweb-import`.
//!
//! Book content is stored as Markdown, but with HTML entities baked in as
//! literal text (`&period;`, `&comma;`, `&ldquo;`, `&rsquor;`, …) because the
//! frontend contenteditable editor produces them on paste and round-trips them
//! through `html_to_markdown` untouched. Every exporter therefore decodes
//! entities first, then renders to the chosen format.
//!
//! Phase 1 implements Markdown. DOCX / EPUB / PDF are wired through the same
//! `export()` entry point and currently return [`ExportError::Unsupported`].

mod docx;
mod epub;
mod html;
mod markdown;
pub mod text;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Docx,
    Epub,
    Pdf,
}

impl ExportFormat {
    /// Parse the `?format=` query value. Returns `None` for unknown values.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "docx" => Some(Self::Docx),
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }

    /// MIME type for the produced file.
    pub fn mime(&self) -> &'static str {
        match self {
            Self::Markdown => "text/markdown; charset=utf-8",
            Self::Docx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            Self::Epub => "application/epub+zip",
            Self::Pdf => "application/pdf",
        }
    }

    /// File extension (no dot).
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Docx => "docx",
            Self::Epub => "epub",
            Self::Pdf => "pdf",
        }
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("export format not yet supported: {0}")]
    Unsupported(&'static str),
    #[error("failed to render export: {0}")]
    Render(String),
}

/// A single chapter to render, already in book order.
pub struct ExportChapter {
    pub title: String,
    /// Raw stored Markdown (HTML entities not yet decoded).
    pub content: String,
}

/// Everything an exporter needs about the book being exported.
pub struct ExportInput {
    pub title: String,
    pub description: String,
    pub chapters: Vec<ExportChapter>,
}

/// Render `input` to `format`, returning the file bytes.
pub fn export(input: &ExportInput, format: ExportFormat) -> Result<Vec<u8>, ExportError> {
    match format {
        ExportFormat::Markdown => Ok(markdown::render(input).into_bytes()),
        ExportFormat::Docx => docx::render(input),
        ExportFormat::Epub => epub::render(input),
        ExportFormat::Pdf => Err(ExportError::Unsupported("pdf")),
    }
}

/// Decode HTML entities the contenteditable editor bakes into stored content
/// as literal text (`&period;` → `.`, `&rsquor;` → `’`, `&ldquo;` → `“`, …).
pub(crate) fn decode_entities(s: &str) -> String {
    html_escape::decode_html_entities(s).into_owned()
}

/// Parse stored content as a rinch-editor `DocNode` if it looks like one.
///
/// Content authored by the editor is DocNode JSON (a top-level `{ … }` object);
/// older content is legacy Markdown (chapters) / HTML (notes) plain text. We
/// only attempt a parse when the trimmed content starts with `{` and it both
/// deserializes and validates against the starter-kit schema — otherwise the
/// caller treats it as legacy.
fn parse_docnode(content: &str) -> Option<rinch_editor_core::Node> {
    if !content.trim_start().starts_with('{') {
        return None;
    }
    let doc: rinch_editor_core::serialize::DocNode = serde_json::from_str(content).ok()?;
    let schema = rinch_editor_core::Schema::starter_kit();
    schema.node_from_doc(&doc).ok()
}

/// Render stored content to Markdown.
///
/// DocNode JSON is rendered via the editor's `doc_to_markdown`; legacy Markdown
/// passes through byte-for-byte unchanged.
///
/// Plain Markdown has no syntax for paragraph alignment, so a DocNode
/// paragraph's `text_align` is intentionally **not** preserved in this output
/// — there is nothing for it to become. It is bridged through as the internal
/// `{align:X}` marker convention already used by the legacy DOCX importer
/// (`plotweb-import/src/docx.rs`) so the DOCX exporter's existing
/// marker-consuming walk (`docx::render_chapter_body`) can recover it; every
/// other consumer of this string (`markdown::render` via
/// `strip_align_markers`) strips those markers back out before they reach a
/// human.
pub(crate) fn content_to_markdown(content: &str) -> String {
    match parse_docnode(content) {
        Some(node) => docnode_to_markdown_with_align_markers(&node),
        None => content.to_string(),
    }
}

/// Render a DocNode `doc` to Markdown one top-level block at a time, so a
/// `paragraph` carrying `text_align: center|right|justify` can be prefixed
/// with an `{align:X}` marker line — see [`content_to_markdown`].
///
/// Rendering block-by-block (via a synthetic single-child `doc`) and joining
/// with the same `"\n\n"` separator `doc_to_markdown` uses internally produces
/// byte-identical output to calling `doc_to_markdown` on the whole tree when no
/// block needs a marker, since each block's own rendering already ends in
/// exactly `"\n\n"` before the (per-block) `trim_end`.
///
/// Only `paragraph` carries the marker: `docx::render_chapter_body`'s marker
/// check only fires at the start of a *paragraph* event, not a heading, so
/// marking a heading here would leak the literal `{align:X}` text into the
/// DOCX output instead of being consumed. Heading alignment remains a known
/// gap for DOCX, same as before this change.
fn docnode_to_markdown_with_align_markers(doc: &rinch_editor_core::Node) -> String {
    doc.content()
        .children()
        .iter()
        .map(|child| {
            let single =
                doc.copy_with_content(rinch_editor_core::Fragment::from_node(child.clone()));
            let md = rinch_editor_core::serialize::doc_to_markdown(&single);
            if child.type_name() == "paragraph"
                && let Some(a @ ("center" | "right" | "justify")) =
                    child.attrs().get_str("text_align")
            {
                format!("{{align:{a}}}\n{md}")
            } else {
                md
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Render stored content to an HTML body fragment (no `<html>`/`<body>` wrapper).
///
/// DocNode JSON is rendered via the editor's schema-driven `node_to_html`;
/// legacy content falls through to the existing Markdown-to-HTML path.
pub(crate) fn content_to_html_fragment(content: &str) -> String {
    match parse_docnode(content) {
        Some(node) => rinch_editor_core::serialize::node_to_html(&node),
        None => html::markdown_to_html_fragment(content),
    }
}

/// Like [`content_to_html_fragment`] but coerces HTML5 void elements to
/// self-closing XHTML form so the output is well-formed enough for EPUB.
///
/// For DocNode content we apply the same void-element coercion that
/// `markdown_to_xhtml_fragment` applies to the legacy path; for legacy content
/// we reuse that function directly so its output is unchanged.
pub(crate) fn content_to_xhtml_fragment(content: &str) -> String {
    html::coerce_void_elements_xhtml(&content_to_html_fragment(content))
}

/// Hand-built DocNode-JSON fixtures shared by this module's tests and by the
/// `docx`/`epub` test modules, for cases `doc_from_markdown` can't produce
/// (there is no Markdown syntax for `text_align`).
#[cfg(test)]
pub(crate) mod test_support {
    /// A one-paragraph doc whose paragraph carries `text_align: <align>`.
    /// Matches the `DocNode` wire shape (`serialize::doc_json::DocNode`):
    /// `{"type": ..., "attrs": {...}, "content": [...], "text": ..., "marks": [...]}`.
    pub(crate) fn aligned_paragraph_json(align: &str, text: &str) -> String {
        format!(
            r#"{{"type":"doc","content":[{{"type":"paragraph","attrs":{{"text_align":"{align}"}},"content":[{{"type":"text","text":"{text}"}}]}}]}}"#
        )
    }
}

#[cfg(test)]
mod docnode_tests {
    use super::test_support::aligned_paragraph_json;
    use super::*;

    /// Build a DocNode-JSON string (as the editor would store) from Markdown.
    fn docnode_json(md: &str) -> String {
        let schema = rinch_editor_core::Schema::starter_kit();
        let node = rinch_editor_core::serialize::doc_from_markdown(&schema, md)
            .expect("markdown parses");
        serde_json::to_string(&node.to_doc().expect("to_doc")).expect("serialize")
    }

    #[test]
    fn docnode_content_renders_to_markdown() {
        let json = docnode_json("# Heading\n\nsome **bold** text");
        let md = content_to_markdown(&json);
        assert!(md.contains("# Heading"), "markdown was: {md}");
        assert!(md.contains("**bold**"), "markdown was: {md}");
    }

    #[test]
    fn docnode_content_renders_to_html_fragment() {
        let json = docnode_json("# Heading\n\nsome **bold** text");
        let html = content_to_html_fragment(&json);
        assert!(html.contains("<h1>"), "html was: {html}");
        assert!(html.contains("<strong>"), "html was: {html}");
    }

    #[test]
    fn centered_paragraph_html_fragment_carries_inline_style() {
        // HTML/EPUB path: rinch's `node_to_html` (called directly, unmodified by
        // us) already turns a `text_align` attr into an inline style — this just
        // guards that our glue doesn't lose or sanitize it away.
        let json = aligned_paragraph_json("center", "Centered");
        let html = content_to_html_fragment(&json);
        assert_eq!(html, r#"<p style="text-align:center">Centered</p>"#);
    }

    #[test]
    fn centered_paragraph_xhtml_fragment_keeps_inline_style() {
        let json = aligned_paragraph_json("right", "Righty");
        let xhtml = content_to_xhtml_fragment(&json);
        assert_eq!(xhtml, r#"<p style="text-align:right">Righty</p>"#);
    }

    #[test]
    fn centered_paragraph_markdown_carries_align_marker_for_docx() {
        // Plain Markdown can't express alignment, but `content_to_markdown` must
        // still bridge it through as the `{align:X}` marker the DOCX exporter's
        // `render_chapter_body` already knows how to consume (see the doc
        // comment on `content_to_markdown`).
        let json = aligned_paragraph_json("center", "Centered");
        let md = content_to_markdown(&json);
        assert_eq!(md, "{align:center}\nCentered");
    }

    #[test]
    fn unaligned_paragraph_markdown_has_no_marker() {
        // A left/default-aligned paragraph must render exactly as before this
        // change — no marker leaks in for the common case.
        let json = docnode_json("just some text");
        let md = content_to_markdown(&json);
        assert_eq!(md, "just some text");
        assert!(!md.contains("{align:"));
    }

    #[test]
    fn multi_block_markdown_with_one_aligned_paragraph_matches_unmarked_blocks() {
        // The marker is inserted per-block without disturbing the surrounding
        // document's structure or spacing (heading, then the centered
        // paragraph, then a plain paragraph — each separated by exactly one
        // blank line, same as `doc_to_markdown` on the whole tree would do).
        let full_json = r#"{"type":"doc","content":[
            {"type":"heading","attrs":{"level":1},"content":[{"type":"text","text":"Title"}]},
            {"type":"paragraph","attrs":{"text_align":"center"},"content":[{"type":"text","text":"Centered"}]},
            {"type":"paragraph","content":[{"type":"text","text":"plain paragraph"}]}
        ]}"#;
        let md = content_to_markdown(full_json);
        assert_eq!(md, "# Title\n\n{align:center}\nCentered\n\nplain paragraph");
    }

    #[test]
    fn a_rich_unaligned_document_renders_exactly_as_the_whole_tree_would() {
        // `content_to_markdown` now renders top-level blocks one at a time so it
        // can slip an `{align:X}` marker in front of an aligned paragraph. That
        // rewrite is only safe if it is a no-op for every document that needs no
        // marker — which is nearly all of them, and all of the legacy ones. This
        // pins that against the unsplit renderer rather than against a literal,
        // so it keeps holding if rinch's Markdown output ever changes shape.
        let source = "# Title\n\nA paragraph with **bold** and *italic*.\n\n                      - first item\n- second item\n\n                      > a quotation\n\n## Subheading\n\nClosing words.";
        let json = docnode_json(source);
        let schema = rinch_editor_core::Schema::starter_kit();
        let node = schema
            .node_from_doc(&serde_json::from_str(&json).expect("json parses"))
            .expect("validates");
        assert_eq!(
            content_to_markdown(&json),
            rinch_editor_core::serialize::doc_to_markdown(&node),
        );
    }

    #[test]
    fn docnode_with_horizontal_rule_renders_to_markdown_thematic_break() {
        // A DocNode `paragraph` / `horizontal_rule` / `paragraph` sequence (the
        // shape the editor stores for a scene break) must come back out as a
        // `---` line, not be dropped by the block-by-block renderer.
        let json = docnode_json("Para one.\n\n---\n\nPara two.");
        let doc: rinch_editor_core::serialize::DocNode =
            serde_json::from_str(&json).expect("valid DocNode JSON");
        assert!(
            doc.content.iter().any(|n| n.node_type == "horizontal_rule"),
            "expected a horizontal_rule node, got: {json}"
        );
        let md = content_to_markdown(&json);
        assert!(md.contains("---"), "markdown was: {md}");
        assert!(md.contains("Para one."), "markdown was: {md}");
        assert!(md.contains("Para two."), "markdown was: {md}");
    }

    #[test]
    fn legacy_markdown_passes_through_unchanged() {
        // Legacy content is not a JSON object, so it must pass through verbatim.
        let legacy = "# Heading\n\nsome **bold** text";
        assert_eq!(content_to_markdown(legacy), legacy);
        // And the HTML path matches the pre-existing legacy renderer exactly.
        assert_eq!(
            content_to_html_fragment(legacy),
            html::markdown_to_html_fragment(legacy)
        );
    }
}
