mod markdown;
mod docx;

use rinch_editor_core::serialize::{DocNode, JsonAttr};
use thiserror::Error;

pub use markdown::normalize_manuscript_paragraphs;

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
            // Decided once, over the whole manuscript, before chapters are
            // split out — see `normalize_manuscript_paragraphs` for why.
            let normalized = normalize_manuscript_paragraphs(&text);
            markdown::split_chapters(&normalized)
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
/// The DOCX importer (`docx::split_chapters`) has no DocNode representation of
/// paragraph alignment to hand over directly, so it instead emits an
/// `{align:X}` marker as its own markdown paragraph immediately before the
/// block it describes (see the comment there). Once markdown parsing has
/// produced the DocNode tree, [`apply_alignment_markers`] walks it, folds each
/// marker's alignment into the `text_align` attr of the sibling that follows
/// it, and drops the marker node — so it never appears as visible text, and
/// `text_align` is present only for `center`/`right`/`justify` (`left` stays
/// the schema's implicit default and is omitted). Markdown-file imports never
/// contain these markers, so the pass is a no-op for them.
///
/// If markdown parsing fails, the raw markdown is returned unchanged: the
/// stored content stays legacy-tolerant and the editor still loads it via its
/// legacy shim.
pub fn markdown_to_docnode_json(md: &str) -> String {
    use rinch_editor_core::Schema;
    use rinch_editor_core::serialize::doc_from_markdown;

    let schema = Schema::starter_kit();
    match doc_from_markdown(&schema, md).and_then(|node| node.to_doc()) {
        Ok(mut doc) => {
            apply_alignment_markers(&mut doc);
            serde_json::to_string(&doc).unwrap_or_else(|_| md.to_string())
        }
        Err(_) => md.to_string(),
    }
}

/// The `text_align` value carried by an `{align:X}` marker, if `text` is
/// exactly one. Kept in sync with the alignments `docx::split_chapters` can
/// produce (`center`, `right`, `justify`); `left` is never emitted because it
/// is the schema's default and would be a no-op.
fn align_marker_value(text: &str) -> Option<&'static str> {
    match text {
        "{align:center}" => Some("center"),
        "{align:right}" => Some("right"),
        "{align:justify}" => Some("justify"),
        _ => None,
    }
}

/// True when `node` is exactly an `{align:X}` marker: a paragraph whose sole
/// content is that one plain (unmarked) text run. Real body text never takes
/// this shape, so the check cannot misfire on genuine content.
fn marker_value(node: &DocNode) -> Option<&'static str> {
    if node.node_type != "paragraph" {
        return None;
    }
    match node.content.as_slice() {
        [only] if only.marks.is_empty() => align_marker_value(only.text.as_deref().unwrap_or("")),
        _ => None,
    }
}

/// Fold `{align:X}` marker paragraphs (see [`markdown_to_docnode_json`]) into
/// the `text_align` attr of the sibling block that immediately follows them,
/// removing the marker nodes themselves. Recurses into container children
/// (list items, blockquotes, …) for robustness, though today's DOCX importer
/// only ever emits markers at chapter top level.
fn apply_alignment_markers(node: &mut DocNode) {
    let mut pending: Option<&'static str> = None;
    let mut kept = Vec::with_capacity(node.content.len());
    for mut child in std::mem::take(&mut node.content) {
        if let Some(align) = marker_value(&child) {
            pending = Some(align);
            continue;
        }
        if let Some(align) = pending.take()
            && matches!(child.node_type.as_str(), "paragraph" | "heading")
        {
            child
                .attrs
                .insert("text_align".to_string(), JsonAttr::Str(align.to_string()));
        }
        apply_alignment_markers(&mut child);
        kept.push(child);
    }
    node.content = kept;
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The `text_align` attr on `node`, or `None` if absent (how "left" is
    /// represented).
    fn node_text_align(node: &DocNode) -> Option<&str> {
        match node.attrs.get("text_align") {
            Some(JsonAttr::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The `text_align` attr of the first node of `node_type` in `doc`.
    fn text_align_of<'a>(doc: &'a DocNode, node_type: &str) -> Option<&'a str> {
        node_text_align(doc.content.iter().find(|n| n.node_type == node_type)?)
    }

    #[test]
    fn markdown_to_docnode_json_strips_align_marker_before_heading() {
        let json = markdown_to_docnode_json("{align:center}\n# Title");
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert!(
            !json.contains("{align:"),
            "align marker leaked into output: {json}"
        );
        assert_eq!(
            text_align_of(&doc, "heading"),
            Some("center"),
            "expected the heading to carry the marker's alignment, got: {json}"
        );
    }

    #[test]
    fn markdown_to_docnode_json_applies_alignment_to_paragraph() {
        // Mirrors the blank-line-separated shape `docx::split_chapters` emits:
        // the marker as its own paragraph, then the aligned paragraph.
        let json = markdown_to_docnode_json("{align:right}\n\nSome paragraph text.");
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert!(
            !json.contains("{align:"),
            "align marker leaked into output: {json}"
        );
        assert_eq!(text_align_of(&doc, "paragraph"), Some("right"));
        assert_eq!(
            doc.content.len(),
            1,
            "the marker paragraph must be removed: {json}"
        );
    }

    #[test]
    fn markdown_to_docnode_json_applies_justify() {
        let json = markdown_to_docnode_json("{align:justify}\n\nJustified text.");
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert_eq!(text_align_of(&doc, "paragraph"), Some("justify"));
    }

    #[test]
    fn markdown_file_scene_break_between_paragraphs_becomes_horizontal_rule_not_heading() {
        // A real CommonMark hazard: `text\n---` (no blank line) parses as a
        // setext H2, so a scene break must stay blank-line separated from the
        // paragraph before it all the way through the manuscript-level
        // `parse_manuscript` -> `markdown_to_docnode_json` pipeline, and must
        // not be swallowed as a chapter boundary along the way either.
        let data = b"# Chapter One\n\nPara one.\n\n---\n\nPara two.";
        let chapters =
            parse_manuscript(data, ImportFormat::Markdown).expect("markdown file parses");
        assert_eq!(chapters.len(), 1, "the --- must not split chapters: {chapters:?}");

        let json = markdown_to_docnode_json(&chapters[0].content);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");
        assert!(
            doc.content.iter().any(|n| n.node_type == "horizontal_rule"),
            "expected a horizontal_rule, got: {json}"
        );
        assert!(
            !doc.content.iter().any(|n| n.node_type == "heading"),
            "a blank-line-separated `---` must not become a setext heading: {json}"
        );
    }

    #[test]
    fn markdown_to_docnode_json_plain_markdown_has_no_text_align() {
        // No markers at all: a plain markdown/text import must be unaffected —
        // no `text_align` attr appears anywhere.
        let json = markdown_to_docnode_json("# Title\n\nJust a normal paragraph.");
        assert!(
            !json.contains("text_align"),
            "unmarked markdown must not gain a text_align attr: {json}"
        );
    }

    /// Count nodes of `node_type` anywhere in `node`'s subtree (inclusive).
    fn count_node_type(node: &DocNode, node_type: &str) -> usize {
        let here = usize::from(node.node_type == node_type);
        here + node
            .content
            .iter()
            .map(|child| count_node_type(child, node_type))
            .sum::<usize>()
    }

    /// A sentence padded well past `LONG_LINE_MIN_CHARS` (100 chars), unique
    /// per call via `n`, so lines don't accidentally collide.
    fn long_line(n: usize) -> String {
        format!(
            "This is prose line number {n} and it just keeps going so that it comfortably clears the long-line character threshold used by the heuristic, yes indeed."
        )
    }

    #[test]
    fn normalize_line_per_paragraph_document_yields_one_paragraph_per_line() {
        let lines: Vec<String> = (0..6).map(long_line).collect();
        let md = lines.join("\n");

        let normalized = normalize_manuscript_paragraphs(&md);
        assert_ne!(normalized, md, "line-per-paragraph style should be detected and rewritten");

        let json = markdown_to_docnode_json(&normalized);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");
        assert_eq!(
            count_node_type(&doc, "paragraph"),
            lines.len(),
            "expected one paragraph node per prose line, got: {json}"
        );
    }

    #[test]
    fn normalize_hard_wrapped_document_is_unchanged() {
        // Three paragraphs, each hard-wrapped at well under 90 columns,
        // blank-line separated — exactly what a conventionally-wrapped
        // manuscript looks like. Note lines *within* a paragraph do touch
        // (single newline, no blank), which is what makes the long-line
        // share the discriminator here, not the pair share.
        let md = "\
The first paragraph wraps across a\n\
few short lines like this one right\n\
here, none of them especially long.\n\
\n\
The second paragraph does the same\n\
thing, staying well under ninety\n\
characters on every single line.\n\
\n\
The third and final paragraph also\n\
wraps this way, blank line before\n\
it and nothing longer than this.";

        let normalized = normalize_manuscript_paragraphs(md);
        assert_eq!(normalized, md, "hard-wrapped prose must pass through unchanged");

        let json = markdown_to_docnode_json(&normalized);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");
        assert_eq!(count_node_type(&doc, "paragraph"), 3);
    }

    #[test]
    fn normalize_blank_line_separated_long_lines_is_unchanged() {
        // Long lines, but already one paragraph per line separated by blank
        // lines — there are no *consecutive* prose line pairs, so this must
        // not be touched (or the touch must be a no-op).
        let lines: Vec<String> = (0..5).map(long_line).collect();
        let md = lines.join("\n\n");

        let normalized = normalize_manuscript_paragraphs(&md);
        assert_eq!(normalized, md, "already blank-line-separated prose must be unchanged");
    }

    #[test]
    fn normalize_line_per_paragraph_document_preserves_other_block_constructs() {
        // Each special construct is set off by blank lines on both sides —
        // realistic even in an otherwise blank-line-free manuscript, and it
        // sidesteps CommonMark lazy-continuation quirks (e.g. a block quote
        // or list absorbing the very next line as a "lazy continuation" of
        // its last paragraph when nothing separates them) that are
        // orthogonal to this heuristic: those lines are never touched by it
        // either way, since they're all classified `Other`. The
        // line-per-paragraph pairs that drive detection live in the
        // untouched prose around them.
        let md = format!(
            "{a}\n{b}\n\n```\ncode line one\ncode line two\n```\n\n{c}\n\n| a | b |\n| - | - |\n| c | d |\n\n{d}\n\n> quote line one\n> quote line two\n> quote line three\n\n{e}\n\n- item one\n- item two\n- item three\n\n{f}\n\nSetext Title\n====\n\n{g}\n{h}  \n{i}\n{j}\n{k}",
            a = long_line(1),
            b = long_line(2),
            c = long_line(3),
            d = long_line(4),
            e = long_line(5),
            f = long_line(6),
            g = long_line(7),
            h = "Short line with a trailing hard break",
            i = long_line(8),
            j = long_line(9),
            k = long_line(10),
        );

        let normalized = normalize_manuscript_paragraphs(&md);
        assert_ne!(normalized, md, "expected this mixed document to be detected as line-per-paragraph");

        // Fenced code block: its lines stay joined, untouched.
        assert!(
            normalized.contains("```\ncode line one\ncode line two\n```"),
            "fenced code block must stay intact: {normalized}"
        );
        // Table: rows stay adjacent. (Full GFM table recognition depends on
        // rinch's markdown importer, not on this pre-pass — it deliberately
        // never touches these lines either way — so this only requires the
        // weaker guarantee the card calls for: the rows are not pulled
        // apart from each other.)
        assert!(
            normalized.contains("| a | b |\n| - | - |\n| c | d |"),
            "table rows must stay adjacent: {normalized}"
        );
        // Blockquote: its 3 lines stay one block.
        assert!(
            normalized.contains("> quote line one\n> quote line two\n> quote line three"),
            "blockquote lines must stay adjacent: {normalized}"
        );
        // List: its 3 items stay adjacent.
        assert!(
            normalized.contains("- item one\n- item two\n- item three"),
            "list items must stay adjacent: {normalized}"
        );
        // Setext heading: title and underline stay adjacent.
        assert!(
            normalized.contains("Setext Title\n===="),
            "setext heading title and underline must stay adjacent: {normalized}"
        );
        // Hard break: the two spaces keep the next line in the same paragraph.
        assert!(
            normalized.contains("Short line with a trailing hard break  \n"),
            "hard break line must keep its trailing spaces: {normalized}"
        );

        let json = markdown_to_docnode_json(&normalized);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert_eq!(count_node_type(&doc, "code_block"), 1, "expected the fence to survive as one code_block: {json}");
        assert_eq!(count_node_type(&doc, "blockquote"), 1, "expected one blockquote: {json}");
        assert_eq!(count_node_type(&doc, "bullet_list"), 1, "expected one bullet_list: {json}");
        assert_eq!(count_node_type(&doc, "list_item"), 3, "expected 3 list_items: {json}");
        assert_eq!(count_node_type(&doc, "heading"), 1, "expected the setext line to become one heading: {json}");
        assert!(
            doc.content.iter().any(|n| n.node_type == "heading"
                && n.content.iter().any(|t| t.text.as_deref() == Some("Setext Title"))),
            "expected a heading titled 'Setext Title': {json}"
        );
        assert_eq!(
            count_node_type(&doc, "hard_break"),
            1,
            "expected exactly one hard_break node, from the two-space line: {json}"
        );
    }

    #[test]
    fn normalize_manuscript_paragraphs_existing_behaviour_still_holds() {
        // Regression guard for the pre-existing scene-break/heading tests
        // above: normalization must not disturb already-correct,
        // blank-line-separated markdown at all.
        let md = "# Chapter One\n\nPara one.\n\n---\n\nPara two.";
        assert_eq!(normalize_manuscript_paragraphs(md), md);
    }

    #[test]
    fn normalize_manuscript_paragraphs_real_sample_chapter_ix() {
        // Runs only if the real sample manuscript is present locally.
        let path = "/home/notyou/Downloads/self-less-legacy.md";
        let Ok(data) = std::fs::read(path) else {
            return;
        };

        let chapters = parse_manuscript(&data, ImportFormat::Markdown).expect("manuscript parses");
        let chapter = chapters
            .iter()
            .find(|c| c.title.contains("Pierce"))
            .expect("chapter IX ~ Pierce present in the sample");

        let json = markdown_to_docnode_json(&chapter.content);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        let paragraph_count = count_node_type(&doc, "paragraph");
        println!("chapter IX ~ Pierce paragraph count: {paragraph_count}");
        assert!(
            paragraph_count > 40,
            "expected more than 40 paragraphs in chapter IX ~ Pierce, got {paragraph_count}"
        );

        let all_text: String = {
            fn collect_text(node: &DocNode, out: &mut String) {
                if let Some(t) = &node.text {
                    out.push_str(t);
                }
                for child in &node.content {
                    collect_text(child, out);
                }
            }
            let mut out = String::new();
            collect_text(&doc, &mut out);
            out
        };
        assert!(
            all_text.contains("I would've come anyway"),
            "expected chapter text to still contain the opening line"
        );
    }

    /// Build a minimal in-memory .docx (a zip with a single `word/document.xml`
    /// entry) from a raw `<w:body>` fragment, the way a real Word document
    /// wraps its paragraphs.
    fn build_docx(body: &str) -> Vec<u8> {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>{body}</w:body>
</w:document>"#
        );

        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("word/document.xml", options).unwrap();
            std::io::Write::write_all(&mut zip, xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        buf
    }

    fn paragraph_xml(jc: Option<&str>, text: &str) -> String {
        let ppr = match jc {
            Some(val) => format!(r#"<w:pPr><w:jc w:val="{val}"/></w:pPr>"#),
            None => String::new(),
        };
        format!(r#"<w:p>{ppr}<w:r><w:t>{text}</w:t></w:r></w:p>"#)
    }

    fn heading_paragraph_xml(text: &str) -> String {
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
        )
    }

    #[test]
    fn docx_import_keeps_paragraph_alignment() {
        let body = format!(
            "{}{}{}{}",
            heading_paragraph_xml("Chapter One"),
            paragraph_xml(Some("center"), "Centered line."),
            paragraph_xml(Some("right"), "Right-aligned line."),
            paragraph_xml(Some("both"), "Justified line."),
        );
        let data = build_docx(&body);

        let chapters = crate::docx::split_chapters(&data).expect("docx parses");
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "Chapter One");

        let json = markdown_to_docnode_json(&chapters[0].content);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert!(
            !json.contains("{align:"),
            "align marker leaked into stored content: {json}"
        );

        let paragraphs: Vec<&DocNode> = doc
            .content
            .iter()
            .filter(|n| n.node_type == "paragraph")
            .collect();
        assert_eq!(paragraphs.len(), 3, "expected 3 paragraphs, got: {json}");

        assert_eq!(node_text_align(paragraphs[0]), Some("center"));
        assert_eq!(node_text_align(paragraphs[1]), Some("right"));
        assert_eq!(node_text_align(paragraphs[2]), Some("justify"));
    }

    #[test]
    fn docx_import_unaligned_paragraph_has_no_text_align() {
        let body = format!(
            "{}{}",
            heading_paragraph_xml("Chapter One"),
            paragraph_xml(None, "Ordinary left-aligned line."),
        );
        let data = build_docx(&body);

        let chapters = crate::docx::split_chapters(&data).expect("docx parses");
        let json = markdown_to_docnode_json(&chapters[0].content);
        assert!(
            !json.contains("text_align"),
            "an unaligned paragraph must not gain a text_align attr: {json}"
        );
    }

    #[test]
    fn docx_import_converts_scene_break_glyphs_to_horizontal_rule() {
        let body = format!(
            "{}{}{}{}{}",
            heading_paragraph_xml("Chapter One"),
            paragraph_xml(None, "Before the break."),
            paragraph_xml(None, "* * *"),
            paragraph_xml(Some("center"), "#"),
            paragraph_xml(None, "After the break."),
        );
        let data = build_docx(&body);

        let chapters = crate::docx::split_chapters(&data).expect("docx parses");
        assert_eq!(chapters.len(), 1);

        // Emitted as its own blank-line-separated block so it can never be
        // read as a setext heading underline for the preceding paragraph.
        assert!(
            chapters[0].content.contains("\n\n---\n\n"),
            "content was: {:?}",
            chapters[0].content
        );
        assert!(
            !chapters[0].content.contains('*') && !chapters[0].content.contains('#'),
            "literal scene-break glyph leaked into stored markdown: {:?}",
            chapters[0].content
        );

        let json = markdown_to_docnode_json(&chapters[0].content);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");

        assert!(
            !json.contains("{align:"),
            "align marker leaked into stored content: {json}"
        );

        let types: Vec<&str> = doc.content.iter().map(|n| n.node_type.as_str()).collect();
        assert_eq!(
            types,
            vec!["paragraph", "horizontal_rule", "horizontal_rule", "paragraph"],
            "expected two horizontal_rule nodes, got: {json}"
        );

        // The centered `#` paragraph's alignment must not leak onto the rule
        // that replaces it, nor onto the paragraph that follows.
        for node in &doc.content {
            assert_eq!(
                node_text_align(node),
                None,
                "no node here should carry text_align: {json}"
            );
        }
    }

    #[test]
    fn docx_import_does_not_convert_text_wrapped_in_asterisks() {
        let body = format!(
            "{}{}",
            heading_paragraph_xml("Chapter One"),
            paragraph_xml(None, "*** important ***"),
        );
        let data = build_docx(&body);

        let chapters = crate::docx::split_chapters(&data).expect("docx parses");
        assert_eq!(chapters[0].content, "*** important ***");

        let json = markdown_to_docnode_json(&chapters[0].content);
        let doc: DocNode = serde_json::from_str(&json).expect("valid DocNode JSON");
        assert!(
            !doc.content.iter().any(|n| n.node_type == "horizontal_rule"),
            "a line with real text must not become a horizontal_rule: {json}"
        );
    }
}
