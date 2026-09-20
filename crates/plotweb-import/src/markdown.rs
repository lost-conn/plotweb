use crate::DetectedChapter;

/// A minimum share of a document's "prose lines" that must be longer than
/// this many characters for the document to be considered NOT hard-wrapped.
/// Prose hard-wrapped at a conventional 72-80 column width almost never
/// produces lines this long, while a manuscript written with one paragraph
/// per raw line (the shape this module corrects) routinely does — a
/// paragraph's whole text sits on a single line.
const LONG_LINE_MIN_CHARS: usize = 100;

/// The minimum share of prose lines that must be longer than
/// [`LONG_LINE_MIN_CHARS`] for a document to be considered "not hard-wrapped".
/// Chosen low enough to tolerate short dialogue-heavy passages while still
/// rejecting genuinely hard-wrapped text, which sits near 0%.
const LONG_LINE_SHARE_THRESHOLD: f64 = 0.25;

/// The minimum share of prose lines that must be immediately followed by
/// another prose line (i.e. separated only by a single `\n`, no blank line
/// between them) for a document to be considered "line-per-paragraph"
/// styled. Combined with [`LONG_LINE_SHARE_THRESHOLD`], this distinguishes:
/// - line-per-paragraph prose: high pair share (paragraphs touch) + high
///   long-line share (each paragraph is one long line) -> detected.
/// - hard-wrapped prose (blank line between paragraphs, ~70-80 col wrap):
///   high pair share too (wrapped lines *within* a paragraph also touch),
///   but near-zero long-line share -> NOT detected.
/// - blank-line-separated prose even with long lines: near-zero pair share
///   (each paragraph line is isolated by blank lines) -> NOT detected.
const CONSECUTIVE_PROSE_PAIR_SHARE_THRESHOLD: f64 = 0.3;

/// How a single line of a manuscript is classified for the purposes of
/// [`normalize_manuscript_paragraphs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    /// An empty (or whitespace-only) line.
    Blank,
    /// A line that reads as ordinary prose: not blank, and not recognized as
    /// part of some other Markdown block construct (heading, code, table,
    /// blockquote, list, or a setext heading underline).
    Prose,
    /// Anything else: headings, fenced/indented code, table rows,
    /// blockquotes, list items, and setext underlines. Never touched by the
    /// blank-line insertion pass.
    Other,
}

/// Classify every line of `lines` into a [`LineKind`], tracking fenced code
/// block state across lines so content inside a fence is never mistaken for
/// prose (or for any other construct — a line that merely *looks* like a
/// table row or list item inside a fence must stay `Other`, i.e. untouched,
/// which falling through to the fence check first guarantees).
fn classify_lines(lines: &[&str]) -> Vec<LineKind> {
    let mut kinds = Vec::with_capacity(lines.len());
    let mut fence_char: Option<char> = None;

    for (i, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim();

        if let Some(ch) = fence_delimiter_char(trimmed) {
            kinds.push(LineKind::Other);
            match fence_char {
                Some(open) if open == ch => fence_char = None,
                Some(_) => {} // different fence char: still inside the block
                None => fence_char = Some(ch),
            }
            continue;
        }
        if fence_char.is_some() {
            kinds.push(LineKind::Other);
            continue;
        }
        if trimmed.is_empty() {
            kinds.push(LineKind::Blank);
            continue;
        }
        if is_indented_code(raw) {
            kinds.push(LineKind::Other);
            continue;
        }
        if is_heading(trimmed)
            || is_table_row(trimmed)
            || is_blockquote(trimmed)
            || is_list_item(trimmed)
        {
            kinds.push(LineKind::Other);
            continue;
        }
        // A setext heading underline (`===`/`---`) only reads as one when it
        // immediately follows a non-blank line (the paragraph it converts
        // into a heading); otherwise a run of `-` is left as ordinary prose
        // (or, standing alone after a blank line, an existing thematic
        // break, which is out of scope here).
        if is_setext_underline_run(trimmed) && i > 0 && kinds[i - 1] != LineKind::Blank {
            kinds.push(LineKind::Other);
            continue;
        }
        kinds.push(LineKind::Prose);
    }

    kinds
}

/// The fence character (`` ` `` or `~`) if `trimmed` opens or closes a fenced
/// code block (3 or more of the same character), else `None`.
fn fence_delimiter_char(trimmed: &str) -> Option<char> {
    for ch in ['`', '~'] {
        if trimmed.chars().take_while(|&c| c == ch).count() >= 3 {
            return Some(ch);
        }
    }
    None
}

/// Indented code: 4+ leading spaces or a leading tab, checked on the raw
/// (untrimmed) line so real leading whitespace is what's tested.
fn is_indented_code(raw: &str) -> bool {
    raw.starts_with("    ") || raw.starts_with('\t')
}

/// ATX heading: 1-6 `#` characters followed by a space or end of line.
fn is_heading(trimmed: &str) -> bool {
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return false;
    }
    matches!(trimmed.as_bytes().get(hashes), None | Some(b' '))
}

/// A table row: contains a pipe. Deliberately simple (matches the card's
/// definition) rather than a full GFM table-row parse.
fn is_table_row(trimmed: &str) -> bool {
    trimmed.contains('|')
}

fn is_blockquote(trimmed: &str) -> bool {
    trimmed.starts_with('>')
}

/// A bullet (`-`, `*`, `+`) or ordered (`1.`, `1)`) list item marker.
fn is_list_item(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    let rest = &trimmed[digits..];
    rest.starts_with(". ") || rest.starts_with(") ")
}

/// A setext underline candidate: a line consisting entirely of `=` or
/// entirely of `-` characters (one or more). Whether it actually *acts* as
/// a setext underline also depends on the preceding line — see the call
/// site in [`classify_lines`].
fn is_setext_underline_run(trimmed: &str) -> bool {
    !trimmed.is_empty()
        && (trimmed.chars().all(|c| c == '=') || trimmed.chars().all(|c| c == '-'))
}

/// True when `raw` ends in a CommonMark hard line break: two or more
/// trailing spaces, or a trailing backslash.
fn ends_with_hard_break(raw: &str) -> bool {
    raw.ends_with("  ") || raw.ends_with('\\')
}

/// Detect whether `lines` (already classified into `kinds`) is written
/// "line-per-paragraph" style: paragraphs separated by a single newline
/// rather than a blank line, with each paragraph long enough that it can't
/// just be ordinary hard-wrapped prose. See the threshold constants above
/// for the exact rule and the reasoning behind each half of it.
fn is_line_per_paragraph_style(lines: &[&str], kinds: &[LineKind]) -> bool {
    let total_prose = kinds.iter().filter(|k| **k == LineKind::Prose).count();
    if total_prose == 0 {
        return false;
    }

    let consecutive_pairs = kinds
        .windows(2)
        .filter(|w| w[0] == LineKind::Prose && w[1] == LineKind::Prose)
        .count();
    let pair_share = consecutive_pairs as f64 / total_prose as f64;

    let long_lines = kinds
        .iter()
        .zip(lines.iter())
        .filter(|(k, line)| **k == LineKind::Prose && line.chars().count() > LONG_LINE_MIN_CHARS)
        .count();
    let long_share = long_lines as f64 / total_prose as f64;

    pair_share >= CONSECUTIVE_PROSE_PAIR_SHARE_THRESHOLD && long_share >= LONG_LINE_SHARE_THRESHOLD
}

/// Insert a blank line between every pair of consecutive non-blank lines
/// that are both classified as prose (see [`classify_lines`]), unless the
/// first of the pair ends in a CommonMark hard break. Every other line is
/// left exactly as it was.
fn insert_paragraph_breaks(lines: &[&str], kinds: &[LineKind]) -> String {
    let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            if kinds[i - 1] == LineKind::Prose
                && kinds[i] == LineKind::Prose
                && !ends_with_hard_break(lines[i - 1])
            {
                out.push('\n');
            }
        }
        out.push_str(line);
    }
    out
}

/// Normalize a manuscript that was authored one paragraph per line, with no
/// blank line separating paragraphs, so that CommonMark parses each line as
/// its own paragraph instead of soft-breaking them all into one.
///
/// This is a heuristic pre-pass meant to run on the *whole* document's text
/// (see [`crate::parse_manuscript`]) before chapters are split out and
/// before the text reaches `doc_from_markdown`: deciding "is this
/// line-per-paragraph?" once, over the whole manuscript, keeps the decision
/// consistent across the book — a short chapter's text alone might not
/// carry enough signal (long lines, consecutive prose pairs) to reliably
/// tell hard-wrapped prose from line-per-paragraph prose on its own.
///
/// The detection ([`is_line_per_paragraph_style`]) and the edit
/// ([`insert_paragraph_breaks`]) both work off the same line classification
/// ([`classify_lines`]), which recognizes headings, fenced/indented code,
/// table rows, blockquotes, list items, and setext heading underlines and
/// never touches them — only runs of plain prose lines get a blank line
/// inserted between them, and never across a CommonMark hard break (two+
/// trailing spaces or a trailing backslash).
///
/// When the document is not detected as line-per-paragraph, this returns
/// `md` unchanged (byte-identical) — no reconstruction happens at all, so
/// there's no risk of e.g. normalizing line endings on an untouched file.
pub fn normalize_manuscript_paragraphs(md: &str) -> String {
    let lines: Vec<&str> = md.split('\n').collect();
    let kinds = classify_lines(&lines);
    if !is_line_per_paragraph_style(&lines, &kinds) {
        return md.to_string();
    }
    insert_paragraph_breaks(&lines, &kinds)
}

/// Split markdown/plain text into chapters based on headings and patterns.
pub fn split_chapters(text: &str) -> Vec<DetectedChapter> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Find all chapter boundary positions and their titles.
    let mut boundaries: Vec<(usize, String)> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(title) = detect_chapter_heading(trimmed) {
            boundaries.push((i, title));
        }
    }

    // If no chapter boundaries found, return entire text as one chapter.
    if boundaries.is_empty() {
        let content = text.trim().to_string();
        if content.is_empty() {
            return Vec::new();
        }
        return vec![DetectedChapter {
            title: "Chapter 1".to_string(),
            content,
        }];
    }

    let mut chapters = Vec::new();

    // Content before the first boundary becomes a preamble chapter (if non-empty).
    if boundaries[0].0 > 0 {
        let preamble: String = lines[..boundaries[0].0].join("\n");
        let preamble = preamble.trim().to_string();
        if !preamble.is_empty() {
            chapters.push(DetectedChapter {
                title: "Preamble".to_string(),
                content: preamble,
            });
        }
    }

    // Each boundary starts a chapter that runs until the next boundary.
    for (idx, (line_num, title)) in boundaries.iter().enumerate() {
        let start = line_num + 1; // skip the heading line itself
        let end = if idx + 1 < boundaries.len() {
            boundaries[idx + 1].0
        } else {
            lines.len()
        };

        let content = if start < end {
            lines[start..end].join("\n").trim().to_string()
        } else {
            String::new()
        };

        chapters.push(DetectedChapter {
            title: title.clone(),
            content,
        });
    }

    chapters
}

/// Detect if a line is a chapter heading. Returns the chapter title if so.
fn detect_chapter_heading(line: &str) -> Option<String> {
    // Markdown heading: # Title (only h1 and h2 are treated as chapter breaks)
    if let Some(rest) = line.strip_prefix("# ") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = line.strip_prefix("## ") {
        return Some(rest.trim().to_string());
    }

    // Common chapter patterns (case-insensitive)
    let upper = line.to_uppercase();

    // "Chapter 1", "Chapter One", "CHAPTER 1: Title", "Chapter 1 - Title"
    if upper.starts_with("CHAPTER ") {
        return Some(clean_chapter_title(line));
    }

    // "Part 1", "Part One", "PART I"
    if upper.starts_with("PART ") && line.len() < 60 {
        return Some(clean_chapter_title(line));
    }

    // "Prologue", "Epilogue", "Interlude"
    let upper_trimmed = upper.trim();
    if matches!(
        upper_trimmed,
        "PROLOGUE" | "EPILOGUE" | "INTERLUDE" | "INTRODUCTION" | "FOREWORD" | "AFTERWORD"
    ) {
        return Some(clean_chapter_title(line));
    }

    // Short ALL-CAPS lines (likely chapter titles) — at least 2 chars, under 60
    if line.len() >= 2
        && line.len() < 60
        && line.chars().all(|c| c.is_uppercase() || c.is_whitespace() || c.is_ascii_punctuation())
        && line.chars().any(|c| c.is_alphabetic())
        && line.chars().filter(|c| c.is_alphabetic()).count() >= 2
    {
        // Avoid matching short words like "OK" or single-word items
        // Only match if it looks like a title (multiple words or matches chapter-like pattern)
        let word_count = line.split_whitespace().count();
        if word_count >= 2 || upper_trimmed.len() >= 5 {
            return Some(titlecase(line.trim()));
        }
    }

    None
}

/// Clean up a chapter title — trim, collapse whitespace.
fn clean_chapter_title(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Convert an ALL-CAPS string to Title Case.
fn titlecase(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    let lower: String = chars.map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
                    format!("{}{}", upper, lower)
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_headings() {
        let text = "# Chapter One\n\nSome text here.\n\n# Chapter Two\n\nMore text.";
        let chapters = split_chapters(text);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Chapter One");
        assert_eq!(chapters[1].title, "Chapter Two");
        assert!(chapters[0].content.contains("Some text"));
    }

    #[test]
    fn test_chapter_keyword() {
        let text = "Chapter 1\n\nFirst chapter.\n\nChapter 2: The Return\n\nSecond chapter.";
        let chapters = split_chapters(text);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Chapter 1");
        assert_eq!(chapters[1].title, "Chapter 2: The Return");
    }

    #[test]
    fn test_no_chapters() {
        let text = "Just a bunch of text\nwith no chapter markers\nat all.";
        let chapters = split_chapters(text);
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "Chapter 1");
    }

    #[test]
    fn test_preamble() {
        let text = "This is the preamble.\n\n# Chapter One\n\nThe story begins.";
        let chapters = split_chapters(text);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Preamble");
        assert_eq!(chapters[1].title, "Chapter One");
    }

    #[test]
    fn test_prologue_epilogue() {
        let text = "Prologue\n\nBefore it all.\n\nChapter 1\n\nThe story.\n\nEpilogue\n\nAfter it all.";
        let chapters = split_chapters(text);
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title, "Prologue");
        assert_eq!(chapters[2].title, "Epilogue");
    }

    #[test]
    fn test_empty() {
        let chapters = split_chapters("");
        assert!(chapters.is_empty());
    }

    #[test]
    fn test_scene_break_is_not_a_chapter_boundary() {
        // `---` has no alphabetic characters, so it never matches the
        // ALL-CAPS-title heuristic (or any other heading pattern here) — a
        // scene break must stay inside its chapter's content, not split it.
        let text = "# Chapter One\n\nSome text.\n\n---\n\nMore text.";
        let chapters = split_chapters(text);
        assert_eq!(chapters.len(), 1, "a scene break must not split chapters: {chapters:?}");
        assert_eq!(chapters[0].title, "Chapter One");
        assert!(chapters[0].content.contains("---"));
    }
}
