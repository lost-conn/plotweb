use crate::decode_entities;
use pulldown_cmark::{Options, Parser, html};

/// Markdown features we enable for every renderer.
fn options() -> Options {
    let mut o = Options::empty();
    o.insert(Options::ENABLE_STRIKETHROUGH);
    o.insert(Options::ENABLE_SMART_PUNCTUATION);
    o
}

/// Decode stored HTML entities, parse the Markdown, and render an HTML body
/// fragment (no `<html>`/`<body>` wrapper). Shared by the EPUB renderer.
pub fn markdown_to_html_fragment(markdown: &str) -> String {
    let decoded = decode_entities(markdown);
    let parser = Parser::new_ext(&decoded, options());
    let mut out = String::new();
    html::push_html(&mut out, parser);
    apply_align_markers(&out)
}

/// Consume custom `{align:X}` block markers in rendered HTML. A marker leads a
/// block (e.g. `<p>{align:center}<br />Text</p>`); we move the alignment onto
/// the enclosing block as a `style="text-align: X;"` attribute (matching the
/// frontend) and drop the marker text plus its trailing `<br />`. Any marker we
/// can't relocate onto an element is at least stripped so no `{align:` literal
/// leaks into output.
fn apply_align_markers(html: &str) -> String {
    let mut out = html.to_string();
    for (marker, css) in [
        ("{align:center}", "center"),
        ("{align:right}", "right"),
        ("{align:justify}", "justify"),
    ] {
        // Relocate a marker that leads a <p>/<hN> block onto that block.
        for tag in ["p", "h1", "h2", "h3", "h4", "h5", "h6"] {
            let open = format!("<{}>", tag);
            // `<p>{align:center}<br />` and `<p>{align:center}`
            let with_br = format!("{}{}<br />", open, marker);
            let styled = format!("<{} style=\"text-align: {};\">", tag, css);
            out = out.replace(&with_br, &styled);
            let bare = format!("{}{}", open, marker);
            out = out.replace(&bare, &styled);
        }
        // Strip any remaining literal markers (e.g. standalone occurrences).
        out = out.replace(marker, "");
    }
    out
}

/// Coerce HTML5 void elements (`<br>`, `<hr>`) into self-closing XHTML form so
/// the fragment is well-formed enough for EPUB. Applied on top of
/// [`markdown_to_html_fragment`] (legacy) or `node_to_html` (DocNode) output by
/// `content_to_xhtml_fragment`.
pub fn coerce_void_elements_xhtml(html: &str) -> String {
    html.replace("<br>", "<br/>")
        .replace("<hr>", "<hr/>")
        .replace("<br />", "<br/>")
        .replace("<hr />", "<hr/>")
}
