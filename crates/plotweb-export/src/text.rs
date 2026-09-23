//! A plain-text view of stored content, for readers that are not people.
//!
//! The MCP endpoint hands chapters and notes to the author's AI agent, searches them,
//! and anchors the agent's review comments to exact passages. All three want the text
//! the author sees on the page — not DocNode JSON, not Markdown syntax, and not the
//! HTML entities the editor bakes into stored content (`&period;`, `&rsquor;`, …; see
//! the crate docs). A quote copied out of [`readable_text`] must be findable verbatim
//! in [`text_blocks`], so both are built from the same blocks.
//!
//! Content comes in three shapes and each is read the way the rest of the crate reads
//! it: DocNode JSON (rendered through the editor's Markdown serializer first), legacy
//! Markdown (chapters) and legacy HTML (notes).

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::{content_to_markdown, decode_entities};

/// What kind of block a [`TextBlock`] was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// A heading, with its level (1–6).
    Heading(u8),
    Paragraph,
    ListItem,
    Quote,
    Code,
}

/// One block of content as plain text: marks stripped, entities decoded, inline
/// breaks kept as `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBlock {
    pub kind: BlockKind,
    pub text: String,
}

/// How to read content that is not DocNode JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Legacy {
    /// Chapters: legacy bodies are Markdown.
    Markdown,
    /// Notes: legacy bodies are HTML.
    Html,
}

/// Split stored content into plain-text blocks, in document order. Empty blocks are
/// dropped.
pub fn text_blocks(content: &str, legacy: Legacy) -> Vec<TextBlock> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if legacy == Legacy::Html && !trimmed.starts_with('{') {
        return html_blocks(trimmed);
    }
    let markdown = strip_align_markers(&content_to_markdown(content));
    markdown_blocks(&markdown)
}

/// Content as readable text: blocks separated by blank lines, headings kept as
/// `#`-prefixed lines, list items as `- ` lines and quotes as `> ` lines. Nothing
/// else is marked up, so any sentence in it can be quoted back verbatim.
pub fn readable_text(content: &str, legacy: Legacy) -> String {
    text_blocks(content, legacy)
        .into_iter()
        .map(|b| match b.kind {
            BlockKind::Heading(level) => {
                format!("{} {}", "#".repeat(level as usize), b.text)
            }
            BlockKind::ListItem => format!("- {}", b.text),
            BlockKind::Quote => format!("> {}", b.text),
            BlockKind::Paragraph | BlockKind::Code => b.text,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Turn Markdown (as an author or an agent would type it) into the editor's DocNode
/// JSON — the shape the editor itself saves a body in. `None` if the editor's
/// schema will not take it.
pub fn markdown_to_docnode_json(markdown: &str) -> Option<String> {
    let schema = rinch_editor_core::Schema::starter_kit();
    let node = rinch_editor_core::serialize::doc_from_markdown(&schema, markdown).ok()?;
    let doc = node.to_doc().ok()?;
    serde_json::to_string(&doc).ok()
}

fn markdown_blocks(markdown: &str) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    // Where the text being gathered will go, and what it is.
    let mut current: Option<(BlockKind, String)> = None;
    let mut in_item = 0usize;
    let mut in_quote = 0usize;

    let flush = |current: &mut Option<(BlockKind, String)>, blocks: &mut Vec<TextBlock>| {
        if let Some((kind, text)) = current.take() {
            let text = decode_entities(text.trim());
            if !text.is_empty() {
                blocks.push(TextBlock { kind, text });
            }
        }
    };

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut current, &mut blocks);
                current = Some((BlockKind::Heading(heading_level(level)), String::new()));
            }
            Event::Start(Tag::Paragraph) => {
                flush(&mut current, &mut blocks);
                let kind = if in_item > 0 {
                    BlockKind::ListItem
                } else if in_quote > 0 {
                    BlockKind::Quote
                } else {
                    BlockKind::Paragraph
                };
                current = Some((kind, String::new()));
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush(&mut current, &mut blocks);
                current = Some((BlockKind::Code, String::new()));
            }
            Event::Start(Tag::Item) => {
                flush(&mut current, &mut blocks);
                in_item += 1;
                // A tight list item carries its text with no paragraph around it.
                current = Some((BlockKind::ListItem, String::new()));
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut current, &mut blocks);
                in_quote += 1;
            }
            Event::End(TagEnd::Item) => {
                flush(&mut current, &mut blocks);
                in_item = in_item.saturating_sub(1);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut current, &mut blocks);
                in_quote = in_quote.saturating_sub(1);
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock) => {
                flush(&mut current, &mut blocks);
            }
            Event::Text(t) | Event::Code(t) => {
                current
                    .get_or_insert_with(|| (BlockKind::Paragraph, String::new()))
                    .1
                    .push_str(&t);
            }
            Event::SoftBreak => {
                if let Some((_, text)) = current.as_mut() {
                    text.push(' ');
                }
            }
            Event::HardBreak => {
                if let Some((_, text)) = current.as_mut() {
                    text.push('\n');
                }
            }
            // Inline HTML (`<br>` and the like) and everything else carries no text.
            _ => {}
        }
    }
    flush(&mut current, &mut blocks);
    blocks
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Legacy HTML note bodies: one block per block-level element, tags stripped.
fn html_blocks(html: &str) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    let mut text = String::new();
    let mut kind = BlockKind::Paragraph;
    let mut chars = html.char_indices().peekable();

    let push = |text: &mut String, kind: BlockKind, blocks: &mut Vec<TextBlock>| {
        let t = decode_entities(&collapse_spaces(text));
        let t = t.trim();
        if !t.is_empty() {
            blocks.push(TextBlock {
                kind,
                text: t.to_string(),
            });
        }
        text.clear();
    };

    while let Some((i, c)) = chars.next() {
        if c != '<' {
            text.push(c);
            continue;
        }
        let end = html[i..].find('>').map(|e| i + e).unwrap_or(html.len() - 1);
        let tag = html[i + 1..end].trim().to_ascii_lowercase();
        while chars.peek().is_some_and(|(j, _)| *j <= end) {
            chars.next();
        }
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        match name.as_str() {
            "br" => text.push(LINE_BREAK),
            "p" | "div" | "li" | "blockquote" | "pre" | "h1" | "h2" | "h3" | "h4" | "h5"
            | "h6" => {
                push(&mut text, kind, &mut blocks);
                kind = if closing {
                    BlockKind::Paragraph
                } else {
                    match name.as_str() {
                        "li" => BlockKind::ListItem,
                        "blockquote" => BlockKind::Quote,
                        "pre" => BlockKind::Code,
                        h if h.starts_with('h') => {
                            BlockKind::Heading(h[1..].parse().unwrap_or(1))
                        }
                        _ => BlockKind::Paragraph,
                    }
                };
            }
            _ => {}
        }
    }
    push(&mut text, kind, &mut blocks);
    blocks
}

/// Stands in for a `<br>` while HTML source whitespace is collapsed.
const LINE_BREAK: char = '\u{2028}';

/// Collapse runs of source whitespace (HTML indentation and newlines) to one space,
/// then turn `<br>` stand-ins into real line breaks.
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for c in s.chars() {
        if c.is_whitespace() && c != LINE_BREAK {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(if c == LINE_BREAK { '\n' } else { c });
            last_space = false;
        }
    }
    out
}

/// Drop `{align:…}` marker lines (see `content_to_markdown`); they are not text.
fn strip_align_markers(content: &str) -> String {
    content
        .lines()
        .filter(|l| {
            !matches!(
                l.trim(),
                "{align:center}" | "{align:right}" | "{align:justify}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docnode_json(md: &str) -> String {
        markdown_to_docnode_json(md).expect("markdown converts")
    }

    #[test]
    fn docnode_content_reads_as_plain_blocks() {
        let json = docnode_json("# Chapter One\n\nThe lantern **guttered** against the *fog*.\n\nSecond para.");
        let blocks = text_blocks(&json, Legacy::Markdown);
        assert_eq!(
            blocks,
            vec![
                TextBlock { kind: BlockKind::Heading(1), text: "Chapter One".into() },
                TextBlock {
                    kind: BlockKind::Paragraph,
                    text: "The lantern guttered against the fog.".into()
                },
                TextBlock { kind: BlockKind::Paragraph, text: "Second para.".into() },
            ]
        );
        assert_eq!(
            readable_text(&json, Legacy::Markdown),
            "# Chapter One\n\nThe lantern guttered against the fog.\n\nSecond para."
        );
    }

    #[test]
    fn baked_in_entities_are_decoded() {
        let md = "She said&comma; &ldquo;don&rsquor;t&rdquo;&period; Tom &amp; Jerry&#39;s.";
        let text = readable_text(md, Legacy::Markdown);
        assert_eq!(text, "She said, \u{201c}don\u{2019}t\u{201d}. Tom & Jerry's.");
    }

    #[test]
    fn entities_inside_docnode_text_are_decoded() {
        let json = r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a &amp; b&period;"}]}]}"#;
        assert_eq!(readable_text(json, Legacy::Markdown), "a & b.");
    }

    #[test]
    fn legacy_markdown_lists_and_quotes() {
        let md = "Intro line\nwraps here.\n\n- one\n- two\n\n> quoted\n";
        let blocks = text_blocks(md, Legacy::Markdown);
        assert_eq!(blocks[0].text, "Intro line wraps here.");
        assert_eq!(blocks[1], TextBlock { kind: BlockKind::ListItem, text: "one".into() });
        assert_eq!(blocks[2], TextBlock { kind: BlockKind::ListItem, text: "two".into() });
        assert_eq!(blocks[3], TextBlock { kind: BlockKind::Quote, text: "quoted".into() });
    }

    #[test]
    fn legacy_html_notes() {
        let html = "<h2>Mira</h2><p>Keeper of the <strong>lamp</strong>&period;</p><ul><li>tall</li></ul>";
        let blocks = text_blocks(html, Legacy::Html);
        assert_eq!(
            blocks,
            vec![
                TextBlock { kind: BlockKind::Heading(2), text: "Mira".into() },
                TextBlock { kind: BlockKind::Paragraph, text: "Keeper of the lamp.".into() },
                TextBlock { kind: BlockKind::ListItem, text: "tall".into() },
            ]
        );
    }

    #[test]
    fn aligned_paragraphs_lose_only_their_marker() {
        let json = crate::test_support::aligned_paragraph_json("center", "Centered");
        assert_eq!(readable_text(&json, Legacy::Markdown), "Centered");
    }

    #[test]
    fn empty_content_has_no_blocks() {
        assert!(text_blocks("", Legacy::Markdown).is_empty());
        assert!(text_blocks("   ", Legacy::Html).is_empty());
        let empty_doc = r#"{"type":"doc","content":[{"type":"paragraph"}]}"#;
        assert_eq!(readable_text(empty_doc, Legacy::Markdown), "");
    }

    #[test]
    fn markdown_round_trips_through_docnode_json() {
        let json = docnode_json("A note about **Mira**.\n\nShe keeps the lamp.");
        assert!(json.starts_with('{'));
        assert_eq!(
            readable_text(&json, Legacy::Html),
            "A note about Mira.\n\nShe keeps the lamp."
        );
    }
}
