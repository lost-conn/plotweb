use crate::{ExportError, ExportInput, content_to_xhtml_fragment, decode_entities};
use epub_builder::{EpubBuilder, EpubContent, ReferenceType, ZipLibrary};

impl From<epub_builder::Error> for ExportError {
    fn from(e: epub_builder::Error) -> Self {
        ExportError::Render(e.to_string())
    }
}

/// Stylesheet shared by every chapter XHTML document. Currently just enough to
/// render a scene-break `<hr>` as a short, centered, understated rule instead
/// of the reading-system default full-width 3D groove.
const STYLESHEET: &str = "hr {\n  border: none;\n  border-top: 1px solid currentColor;\n  width: 30%;\n  margin: 2em auto;\n  opacity: .5;\n}\n";

/// Render the manuscript as an EPUB: one XHTML document per chapter, in book
/// order, with an auto-generated table of contents.
pub fn render(input: &ExportInput) -> Result<Vec<u8>, ExportError> {
    let mut builder = EpubBuilder::new(ZipLibrary::new()?)?;
    builder.metadata("title", &input.title)?;
    builder.metadata("generator", "PlotWeb")?;
    if !input.description.trim().is_empty() {
        builder.metadata("description", input.description.trim())?;
    }
    builder.stylesheet(STYLESHEET.as_bytes())?;

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
<head><meta charset=\"utf-8\"/><title>{t}</title>\n\
<link rel=\"stylesheet\" type=\"text/css\" href=\"stylesheet.css\"/></head>\n\
<body>\n<h1>{t}</h1>\n{body}\n</body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExportChapter;
    use crate::test_support::aligned_paragraph_json;
    use std::io::Read;

    #[test]
    fn centered_paragraph_survives_into_the_epub_chapter_xhtml() {
        let input = ExportInput {
            title: "Book".into(),
            description: String::new(),
            chapters: vec![ExportChapter {
                title: "One".into(),
                content: aligned_paragraph_json("center", "Centered"),
            }],
        };
        let bytes = render(&input).expect("epub renders");

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid zip");
        let mut xhtml = String::new();
        zip.by_name("OEBPS/chapter_1.xhtml")
            .expect("chapter_1.xhtml present")
            .read_to_string(&mut xhtml)
            .expect("readable xhtml");

        assert!(
            xhtml.contains(r#"<p style="text-align:center">Centered</p>"#),
            "chapter xhtml was: {xhtml}"
        );
    }

    #[test]
    fn scene_break_renders_as_self_closing_hr_linked_to_a_stylesheet() {
        let input = ExportInput {
            title: "Book".into(),
            description: String::new(),
            chapters: vec![ExportChapter {
                title: "One".into(),
                content: "Para one.\n\n---\n\nPara two.".into(),
            }],
        };
        let bytes = render(&input).expect("epub renders");

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid zip");

        let mut xhtml = String::new();
        zip.by_name("OEBPS/chapter_1.xhtml")
            .expect("chapter_1.xhtml present")
            .read_to_string(&mut xhtml)
            .expect("readable xhtml");
        assert!(xhtml.contains("<hr/>"), "chapter xhtml was: {xhtml}");
        assert!(
            xhtml.contains(r#"<link rel="stylesheet" type="text/css" href="stylesheet.css"/>"#),
            "chapter xhtml was: {xhtml}"
        );

        let mut css = String::new();
        zip.by_name("OEBPS/stylesheet.css")
            .expect("stylesheet.css present")
            .read_to_string(&mut css)
            .expect("readable css");
        assert!(css.contains("hr {"), "stylesheet.css was: {css}");
        assert!(
            css.contains("border-top: 1px solid currentColor"),
            "stylesheet.css was: {css}"
        );
    }
}
