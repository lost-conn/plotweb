use std::io::Cursor;

use crate::{ExportError, ExportInput, decode_entities};
use docx_rs::{BreakType, Docx, Paragraph, Run};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Render the manuscript as a DOCX. Each chapter starts with a `Heading1`
/// paragraph (the chapter title), followed by its body rendered from Markdown:
/// paragraphs, headings, and inline bold/italic/strikethrough.
pub fn render(input: &ExportInput) -> Result<Vec<u8>, ExportError> {
    let mut docx = Docx::new();

    for ch in &input.chapters {
        let title = decode_entities(&ch.title);
        docx = docx.add_paragraph(
            Paragraph::new()
                .style("Heading1")
                .add_run(Run::new().add_text(title.trim())),
        );
        docx = render_chapter_body(docx, &ch.content);
    }

    let mut cursor = Cursor::new(Vec::new());
    docx.build()
        .pack(&mut cursor)
        .map_err(|e| ExportError::Render(e.to_string()))?;
    Ok(cursor.into_inner())
}

/// Walk a chapter's Markdown and append its block elements to the document.
fn render_chapter_body(mut docx: Docx, markdown: &str) -> Docx {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);
    let decoded = decode_entities(markdown);

    let mut cur = Paragraph::new();
    let mut bold = false;
    let mut italic = false;

    for ev in Parser::new_ext(&decoded, opts) {
        match ev {
            Event::Start(Tag::Paragraph) => {
                cur = Paragraph::new();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                cur = Paragraph::new().style(heading_style(level));
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Heading(_)) => {
                docx = docx.add_paragraph(std::mem::replace(&mut cur, Paragraph::new()));
            }
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Text(t) | Event::Code(t) => {
                let mut run = Run::new().add_text(t.as_ref());
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                cur = cur.add_run(run);
            }
            Event::SoftBreak => {
                cur = cur.add_run(Run::new().add_text(" "));
            }
            Event::HardBreak => {
                cur = cur.add_run(Run::new().add_break(BreakType::TextWrapping));
            }
            _ => {}
        }
    }

    docx
}

fn heading_style(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "Heading1",
        HeadingLevel::H2 => "Heading2",
        HeadingLevel::H3 => "Heading3",
        HeadingLevel::H4 => "Heading4",
        HeadingLevel::H5 => "Heading5",
        HeadingLevel::H6 => "Heading6",
    }
}
