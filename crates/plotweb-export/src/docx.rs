use std::io::Cursor;

use crate::{ExportError, ExportInput, decode_entities};
use docx_rs::{AlignmentType, BreakType, Docx, Paragraph, Run};
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
    let mut underline = false;
    let mut highlight = false;
    // Alignment captured from a `{align:X}` marker paragraph, applied to the
    // NEXT real paragraph instead of being emitted as literal text.
    let mut pending_align: Option<AlignmentType> = None;
    // True at the very start of a paragraph (no runs emitted yet) so a leading
    // `{align:X}` marker line can be consumed instead of rendered.
    let mut at_para_start = false;
    // Set when the just-consumed text was an align marker, so the following
    // soft break (the marker's trailing newline) is also swallowed.
    let mut skip_next_softbreak = false;
    // Whether the current paragraph has received any run. A paragraph that only
    // carried a standalone `{align:X}` marker is dropped (alignment carries to
    // the next paragraph) rather than emitted as an empty paragraph.
    let mut para_has_content = false;
    // Captured href of an open <a> tag; following text gets the URL appended.
    let mut link_href: Option<String> = None;

    for ev in Parser::new_ext(&decoded, opts) {
        match ev {
            Event::Start(Tag::Paragraph) => {
                cur = Paragraph::new();
                at_para_start = true;
                skip_next_softbreak = false;
                para_has_content = false;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                cur = Paragraph::new().style(heading_style(level));
                at_para_start = false;
                para_has_content = true;
            }
            Event::End(TagEnd::Paragraph) => {
                // A paragraph that only held an alignment marker has no content;
                // drop it but keep the alignment pending for the next paragraph.
                if !para_has_content && pending_align.is_some() {
                    cur = Paragraph::new();
                    continue;
                }
                let mut p = std::mem::replace(&mut cur, Paragraph::new());
                if let Some(a) = pending_align.take() {
                    p = p.align(a);
                }
                docx = docx.add_paragraph(p);
            }
            Event::End(TagEnd::Heading(_)) => {
                docx = docx.add_paragraph(std::mem::replace(&mut cur, Paragraph::new()));
            }
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Html(h) | Event::InlineHtml(h) => {
                // Stored inline tags pulldown-cmark passes through verbatim.
                let tag = h.as_ref().trim().to_lowercase();
                if tag.starts_with("<u>") || tag.starts_with("<ins>") {
                    underline = true;
                } else if tag.starts_with("</u>") || tag.starts_with("</ins>") {
                    underline = false;
                } else if tag.starts_with("<mark") {
                    highlight = true;
                } else if tag.starts_with("</mark>") {
                    highlight = false;
                } else if tag.starts_with("<a ") || tag.starts_with("<a>") {
                    link_href = extract_href(h.as_ref());
                } else if tag.starts_with("</a>") {
                    if let Some(href) = link_href.take() {
                        let run = Run::new().add_text(format!(" ({})", href));
                        cur = cur.add_run(run);
                        para_has_content = true;
                    }
                }
                at_para_start = false;
                // Other tags (e.g. <sub>/<sup>) are stripped; their inner text
                // still arrives as Text events and is kept.
            }
            Event::Text(t) | Event::Code(t) => {
                // A leading `{align:X}` marker line sets the paragraph alignment
                // and is not rendered as text.
                if at_para_start {
                    if let Some(a) = parse_align_marker(t.trim()) {
                        pending_align = Some(a);
                        at_para_start = false;
                        skip_next_softbreak = true;
                        continue;
                    }
                }
                at_para_start = false;
                let mut run = Run::new().add_text(t.as_ref());
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                if underline {
                    run = run.underline("single");
                }
                if highlight {
                    run = run.highlight("yellow");
                }
                cur = cur.add_run(run);
                para_has_content = true;
            }
            Event::SoftBreak => {
                if skip_next_softbreak {
                    skip_next_softbreak = false;
                    continue;
                }
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

/// Map a lone `{align:X}` marker line to a docx paragraph alignment.
fn parse_align_marker(line: &str) -> Option<AlignmentType> {
    match line {
        "{align:center}" => Some(AlignmentType::Center),
        "{align:right}" => Some(AlignmentType::Right),
        "{align:justify}" => Some(AlignmentType::Both),
        _ => None,
    }
}

/// Pull the URL out of an `<a href="...">` opening tag.
fn extract_href(tag: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let idx = lower.find("href")?;
    let rest = &tag[idx + 4..];
    let eq = rest.find('=')?;
    let after = rest[eq + 1..].trim_start();
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = after[1..].find(quote)?;
        Some(after[1..1 + end].to_string())
    } else {
        // Unquoted href: take up to whitespace or '>'.
        let end = after.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(after.len());
        Some(after[..end].to_string())
    }
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
