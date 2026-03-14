use crate::DetectedChapter;

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
}
