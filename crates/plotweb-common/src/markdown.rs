//! The canonical PlotWeb legacy-Markdown → HTML converter.
//!
//! This is the **single** line-based Markdown converter shared by the editor
//! (the frontend loads legacy chapter bodies through it before handing the HTML
//! to the editor's `load_html`) and the migration audit (`plotweb-crdt` converts
//! legacy content the same way before round-tripping it onto the CRDT). Keeping
//! one copy guarantees the migration sees byte-for-byte what the editor sees.
//!
//! It is deliberately **line-based**: each non-empty source line becomes its own
//! block, so paragraphs are *not* collapsed together (unlike a CommonMark
//! converter). That paragraph fidelity is the whole point — real books store one
//! paragraph per line — so this logic must stay exactly as-is.
//!
//! Pure `std` string work: no dependencies, compiles on every target (incl.
//! `wasm32`).

/// Append a `</ul>` or `</ol>` closing tag without allocating a temporary.
fn push_close_list(html: &mut String, list_type: &str) {
    html.push_str("</");
    html.push_str(list_type);
    html.push('>');
}

/// Append `<tag{attr}>{inline_md(rest)}</tag>` directly into `html`, avoiding the
/// intermediate `format!` String per line.
fn push_block(html: &mut String, tag: &str, attr: &str, rest: &str) {
    html.push('<');
    html.push_str(tag);
    html.push_str(attr);
    html.push('>');
    html.push_str(&inline_md(rest));
    html.push_str("</");
    html.push_str(tag);
    html.push('>');
}

/// Simple markdown to HTML converter for loading content.
// The nested `if let` for the `{align:…}` marker is kept as-is (not collapsed):
// this is the canonical converter that must stay byte-for-byte identical to the
// editor's copy, so behavior and source both match exactly.
#[allow(clippy::collapsible_if)]
pub fn markdown_to_html(md: &str) -> String {
    let mut html = String::with_capacity(md.len() + md.len() / 4);
    let mut in_list = false;
    let mut list_type = "";
    let mut pending_align: Option<&str> = None;

    for line in md.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if in_list {
                push_close_list(&mut html, list_type);
                in_list = false;
            }
            continue;
        }

        // Check for alignment marker: {align:center}, {align:right}, {align:justify}
        if let Some(rest) = trimmed.strip_prefix("{align:") {
            if let Some(align) = rest.strip_suffix('}') {
                pending_align = match align {
                    "center" => Some("center"),
                    "right" => Some("right"),
                    "justify" => Some("justify"),
                    _ => None,
                };
                continue;
            }
        }

        let style_attr = if let Some(align) = pending_align.take() {
            format!(" style=\"text-align: {};\"", align)
        } else {
            String::new()
        };

        // Headings
        if let Some(rest) = trimmed.strip_prefix("### ") {
            if in_list { push_close_list(&mut html, list_type); in_list = false; }
            push_block(&mut html, "h3", &style_attr, rest);
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            if in_list { push_close_list(&mut html, list_type); in_list = false; }
            push_block(&mut html, "h2", &style_attr, rest);
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            if in_list { push_close_list(&mut html, list_type); in_list = false; }
            push_block(&mut html, "h1", &style_attr, rest);
        } else if let Some(rest) = trimmed.strip_prefix("> ") {
            if in_list { push_close_list(&mut html, list_type); in_list = false; }
            html.push_str("<blockquote><p");
            html.push_str(&style_attr);
            html.push('>');
            html.push_str(&inline_md(rest));
            html.push_str("</p></blockquote>");
        } else if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            if !in_list || list_type != "ul" {
                if in_list { push_close_list(&mut html, list_type); }
                html.push_str("<ul>");
                in_list = true;
                list_type = "ul";
            }
            push_block(&mut html, "li", "", rest);
        } else if trimmed.len() > 2 && trimmed.as_bytes()[0].is_ascii_digit() && trimmed.contains(". ") {
            let rest = &trimmed[trimmed.find(". ").unwrap() + 2..];
            if !in_list || list_type != "ol" {
                if in_list { push_close_list(&mut html, list_type); }
                html.push_str("<ol>");
                in_list = true;
                list_type = "ol";
            }
            push_block(&mut html, "li", "", rest);
        } else if trimmed.starts_with("![") {
            // Image: ![alt](url)
            if in_list { push_close_list(&mut html, list_type); in_list = false; }
            if let Some(img_html) = parse_md_image(trimmed) {
                html.push_str(&img_html);
            } else {
                push_block(&mut html, "p", &style_attr, trimmed);
            }
        } else if trimmed == "---" || trimmed == "***" {
            if in_list { push_close_list(&mut html, list_type); in_list = false; }
            html.push_str("<hr>");
        } else {
            if in_list { push_close_list(&mut html, list_type); in_list = false; }
            push_block(&mut html, "p", &style_attr, trimmed);
        }
    }

    if in_list {
        push_close_list(&mut html, list_type);
    }

    if html.is_empty() {
        html.push_str("<p></p>");
    }

    html
}

/// Wrap each `marker`-delimited pair in `open`/`close`, in a single left-to-right
/// pass. Equivalent to repeatedly finding the next pair, but O(n) instead of the
/// O(n²) "rebuild the whole string per match" approach: each byte of `input` is
/// scanned and copied at most once. An unmatched opening marker (no closing
/// marker after it) stops processing and emits the remainder verbatim, matching
/// the original `break`-on-no-close behavior.
fn replace_pair(input: &str, marker: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let Some(start) = rest.find(marker) else {
            out.push_str(rest);
            break;
        };
        let after = &rest[start + marker.len()..];
        let Some(end) = after.find(marker) else {
            // No closing marker — leave this marker and the remainder untouched.
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str(open);
        out.push_str(&after[..end]);
        out.push_str(close);
        rest = &after[end + marker.len()..];
    }
    out
}

/// Process inline markdown: bold, italic, code, strikethrough.
///
/// Order matters: bold (`**`) is resolved before italic (`*`) so a `**bold**`
/// run isn't misread as two italics, mirroring the original sequential passes.
pub fn inline_md(text: &str) -> String {
    let result = replace_pair(text, "**", "<strong>", "</strong>");
    let result = replace_pair(&result, "*", "<em>", "</em>");
    let result = replace_pair(&result, "`", "<code>", "</code>");
    replace_pair(&result, "~~", "<s>", "</s>")
}

/// Parse a markdown image `![alt](url)` and return an HTML `<img>` tag.
fn parse_md_image(text: &str) -> Option<String> {
    let rest = text.strip_prefix("![")?;
    let alt_end = rest.find("](")?;
    let alt = &rest[..alt_end];
    let after = &rest[alt_end + 2..];
    let url_end = after.find(')')?;
    let url = &after[..url_end];
    Some(format!("<img src=\"{}\" alt=\"{}\">", url, alt))
}
