use crate::{ExportInput, content_to_markdown, decode_entities};

/// Render the manuscript as a single Markdown document: one `# Title` heading
/// per chapter, followed by its entity-decoded body.
///
/// Using a top-level `#` per chapter mirrors the import chapter detector
/// (which treats a level-1 heading as a chapter boundary), so a file exported
/// here round-trips cleanly back through import. The book title/description are
/// intentionally omitted so re-import doesn't pick the title up as a chapter.
pub fn render(input: &ExportInput) -> String {
    let mut out = String::new();
    for (i, ch) in input.chapters.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str("# ");
        out.push_str(decode_entities(&ch.title).trim());
        out.push_str("\n\n");
        out.push_str(
            strip_align_markers(&decode_entities(&content_to_markdown(&ch.content))).trim_end(),
        );
    }
    out.push('\n');
    out
}

/// Drop any line that is exactly a block alignment marker (`{align:center}`,
/// `{align:right}`, `{align:justify}`). Plain Markdown can't express paragraph
/// alignment, so these internal markers would otherwise leak into output as
/// literal garbage text.
fn strip_align_markers(content: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for line in content.lines() {
        if is_align_marker(line) {
            continue;
        }
        if !first {
            out.push('\n');
        }
        out.push_str(line);
        first = false;
    }
    out
}

/// True when `line` is exactly one of the recognized alignment markers.
fn is_align_marker(line: &str) -> bool {
    matches!(
        line.trim(),
        "{align:center}" | "{align:right}" | "{align:justify}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExportChapter;

    fn input(chapters: Vec<(&str, &str)>) -> ExportInput {
        ExportInput {
            title: "Book".into(),
            description: String::new(),
            chapters: chapters
                .into_iter()
                .map(|(t, c)| ExportChapter {
                    title: t.into(),
                    content: c.into(),
                })
                .collect(),
        }
    }

    #[test]
    fn decodes_entities_in_body_and_title() {
        let md = render(&input(vec![(
            "Chapter One&colon; Dawn",
            "Destroy the Machine&period; Liberate the people&period;",
        )]));
        assert!(md.contains("# Chapter One: Dawn"));
        assert!(md.contains("Destroy the Machine. Liberate the people."));
        assert!(!md.contains("&period;"));
    }

    #[test]
    fn separates_chapters_with_blank_line() {
        let md = render(&input(vec![("One", "a"), ("Two", "b")]));
        assert_eq!(md, "# One\n\na\n\n# Two\n\nb\n");
    }

    #[test]
    fn strips_alignment_markers() {
        let md = render(&input(vec![(
            "One",
            "{align:center}\nCentered line\n\n{align:right}\nRight line",
        )]));
        assert!(!md.contains("{align:"));
        assert!(md.contains("Centered line"));
        assert!(md.contains("Right line"));
    }
}
