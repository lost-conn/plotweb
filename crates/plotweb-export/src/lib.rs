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
