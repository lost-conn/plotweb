mod markdown;
mod docx;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("unsupported file format: {0}")]
    UnsupportedFormat(String),
    #[error("failed to read docx: {0}")]
    DocxError(String),
    #[error("file is empty or contains no text")]
    EmptyFile,
}

/// A detected chapter from an imported manuscript.
#[derive(Debug, Clone)]
pub struct DetectedChapter {
    pub title: String,
    pub content: String,
}

/// Supported import formats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImportFormat {
    Markdown,
    Docx,
}

impl ImportFormat {
    pub fn from_filename(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".txt") {
            Some(Self::Markdown)
        } else if lower.ends_with(".docx") {
            Some(Self::Docx)
        } else {
            None
        }
    }
}

/// Parse a manuscript file into chapters.
///
/// If no chapter boundaries are detected, the entire content becomes a single
/// chapter titled "Chapter 1".
pub fn parse_manuscript(
    data: &[u8],
    format: ImportFormat,
) -> Result<Vec<DetectedChapter>, ImportError> {
    let chapters = match format {
        ImportFormat::Markdown => {
            let text = String::from_utf8_lossy(data);
            markdown::split_chapters(&text)
        }
        ImportFormat::Docx => docx::split_chapters(data)?,
    };

    if chapters.is_empty() {
        return Err(ImportError::EmptyFile);
    }

    Ok(chapters)
}

/// Convert a detected Markdown chapter body into the stored DocNode-JSON shape
/// used by the rinch editor.
///
/// `{align:X}` marker lines (emitted by the DOCX importer to carry paragraph
/// alignment) have no DocNode representation, so they are stripped before
/// parsing — alignment is a known gap, tracked separately. If markdown parsing
/// fails, the raw markdown is returned unchanged: the stored content stays
/// legacy-tolerant and the editor still loads it via its legacy shim.
pub fn markdown_to_docnode_json(md: &str) -> String {
    use rinch_editor_core::Schema;
    use rinch_editor_core::serialize::doc_from_markdown;

    // Strip lines that are exactly an alignment marker (trimmed). They have no
    // DocNode representation and would otherwise appear as literal text.
    let stripped: String = md
        .lines()
        .filter(|line| !is_align_marker(line.trim()))
        .collect::<Vec<_>>()
        .join("\n");

    let schema = Schema::starter_kit();
    match doc_from_markdown(&schema, &stripped) {
        Ok(node) => node
            .to_doc()
            .ok()
            .and_then(|d| serde_json::to_string(&d).ok())
            .unwrap_or_else(|| md.to_string()),
        Err(_) => md.to_string(),
    }
}

/// True when `line` is exactly one of the recognized block alignment markers.
fn is_align_marker(line: &str) -> bool {
    matches!(
        line,
        "{align:center}" | "{align:right}" | "{align:justify}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rinch_editor_core::serialize::DocNode;

    #[test]
    fn markdown_to_docnode_json_heading_and_bold() {
        let json = markdown_to_docnode_json("# Title\n\nsome **bold** text");
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert_eq!(doc.node_type, "doc");

        // A heading child.
        assert!(
            doc.content.iter().any(|n| n.node_type == "heading"),
            "expected a heading node, got: {json}"
        );

        // A paragraph containing a text node carrying a `bold` mark.
        let has_bold = doc.content.iter().any(|n| {
            n.node_type == "paragraph"
                && n.content
                    .iter()
                    .any(|t| t.marks.iter().any(|m| m.mark_type == "bold"))
        });
        assert!(has_bold, "expected a bold mark, got: {json}");
    }

    #[test]
    fn markdown_to_docnode_json_strips_align_marker() {
        let json = markdown_to_docnode_json("{align:center}\n# Title");
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert!(
            !json.contains("{align:"),
            "align marker leaked into output: {json}"
        );
        assert!(
            doc.content.iter().any(|n| n.node_type == "heading"),
            "expected a heading node, got: {json}"
        );
    }
}
