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
    out
}

/// Like [`markdown_to_html_fragment`] but coerces HTML5 void elements into
/// self-closing XHTML form so the output is well-formed enough for EPUB.
pub fn markdown_to_xhtml_fragment(markdown: &str) -> String {
    let html = markdown_to_html_fragment(markdown);
    html.replace("<br>", "<br/>")
        .replace("<hr>", "<hr/>")
        .replace("<br />", "<br/>")
        .replace("<hr />", "<hr/>")
}
