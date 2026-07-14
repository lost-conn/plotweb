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
pub(crate) fn content_to_markdown(content: &str) -> String {
    match parse_docnode(content) {
        Some(node) => rinch_editor_core::serialize::doc_to_markdown(&node),
        None => content.to_string(),
    }
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

#[cfg(test)]
mod docnode_tests {
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
