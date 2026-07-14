use crate::{ExportError, ExportInput, content_to_xhtml_fragment, decode_entities};
use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

impl From<epub_builder::Error> for ExportError {
    fn from(e: epub_builder::Error) -> Self {
        ExportError::Render(e.to_string())
    }
}

/// Render the manuscript as an EPUB: one XHTML document per chapter, in book
/// order, with an auto-generated table of contents.
pub fn render(input: &ExportInput) -> Result<Vec<u8>, ExportError> {
    let mut builder = EpubBuilder::new(ZipLibrary::new()?)?;
    builder.metadata("title", &input.title)?;
    builder.metadata("generator", "PlotWeb")?;
    if !input.description.trim().is_empty() {
        builder.metadata("description", input.description.trim())?;
    }

    for (i, ch) in input.chapters.iter().enumerate() {
        let title = decode_entities(&ch.title);
        let title = title.trim();
        let body = content_to_xhtml_fragment(&ch.content);
        let doc = wrap_xhtml(title, &body);
        let content = EpubContent::new(format!("chapter_{}.xhtml", i + 1), doc.as_bytes())
            .title(title)
            .reftype(ReferenceType::Text);
        builder.add_content(content)?;
    }

    let mut buf = Vec::new();
    builder.generate(&mut buf)?;
    Ok(buf)
}

/// Wrap a chapter body fragment in a minimal, well-formed XHTML document.
fn wrap_xhtml(title: &str, body: &str) -> String {
    let t = html_escape::encode_text(title);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\">\n\
<head><meta charset=\"utf-8\"/><title>{t}</title></head>\n\
<body>\n<h1>{t}</h1>\n{body}\n</body>\n</html>\n"
    )
}
