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
