use rinch::prelude::*;
use rinch_core::Signal;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// Call document.execCommand(command, false, value) via JS.
    #[wasm_bindgen(js_namespace = document, js_name = execCommand, catch)]
    fn exec_command_js(command: &str, show_ui: bool, value: &str) -> Result<bool, JsValue>;

    #[wasm_bindgen(js_namespace = document, js_name = queryCommandState, catch)]
    fn query_command_state_js(command: &str) -> Result<bool, JsValue>;

    #[wasm_bindgen(js_namespace = document, js_name = queryCommandValue, catch)]
    fn query_command_value_js(command: &str) -> Result<String, JsValue>;
}

fn query_cmd_state(cmd: &str) -> bool {
    query_command_state_js(cmd).unwrap_or(false)
}

fn query_cmd_value(cmd: &str) -> String {
    query_command_value_js(cmd).unwrap_or_default()
}

/// Execute a browser execCommand on the document (for contenteditable formatting).
pub fn exec_cmd(command: &str) {
    exec_command_js(command, false, "").ok();
}

/// Execute a browser execCommand with a value argument.
pub fn exec_cmd_val(command: &str, value: &str) {
    exec_command_js(command, false, value).ok();
}

/// Editor CSS — focused writing environment with semantic colors.
pub const EDITOR_CSS: &str = r#"
.editor-layout {
    display: flex;
    flex-direction: column;
    height: 100%;
}

.editor-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 20px;
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

.editor-topbar-left {
    display: flex;
    align-items: center;
    gap: 12px;
}

.toolbar {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 2px;
    padding: 6px 20px;
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
    overflow-x: auto;
}

.toolbar-separator {
    width: 1px;
    height: 20px;
    background: var(--rinch-color-border);
    margin: 0 6px;
    flex-shrink: 0;
}

.editor-scroll {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    background: var(--rinch-color-body);
}

.editor-content {
    min-height: 100%;
    max-width: 720px;
    margin: 0 auto;
    padding: 48px 48px;
    font-size: 16px;
    line-height: 1.8;
    color: var(--rinch-color-text);
    outline: none;
    cursor: text;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
}

.editor-content p { margin: 0 0 8px 0; }
.editor-content h1 { font-size: 2em; font-weight: 700; margin: 32px 0 12px 0; color: var(--rinch-color-text); }
.editor-content h2 { font-size: 1.5em; font-weight: 700; margin: 28px 0 10px 0; color: var(--rinch-color-text); }
.editor-content h3 { font-size: 1.25em; font-weight: 600; margin: 24px 0 8px 0; color: var(--rinch-color-text); }
.editor-content h4 { font-size: 1.1em; font-weight: 600; margin: 16px 0 6px 0; color: var(--rinch-color-text); }
.editor-content h5 { font-size: 1em; font-weight: 600; margin: 12px 0 4px 0; color: var(--rinch-color-dimmed); }
.editor-content h6 { font-size: 0.9em; font-weight: 600; margin: 12px 0 4px 0; color: var(--rinch-color-dimmed); }

.editor-content blockquote {
    border-left: 3px solid var(--rinch-color-teal-8);
    padding-left: 16px;
    margin: 16px 0;
    color: var(--rinch-color-dimmed);
}

.editor-content pre {
    background: var(--pw-color-deep);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    padding: 12px 16px;
    margin: 16px 0;
    font-family: monospace;
    font-size: 14px;
    overflow-x: auto;
}

.editor-content code {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 0.9em;
    color: var(--rinch-color-teal-4);
}

.editor-content pre code {
    background: none;
    border: none;
    padding: 0;
    border-radius: 0;
    color: inherit;
}

.editor-content ul, .editor-content ol {
    margin: 8px 0;
    padding-left: 24px;
}

.editor-content li { margin: 4px 0; }

.editor-content hr {
    border: none;
    border-top: 1px solid var(--rinch-color-border);
    margin: 24px 0;
}

.editor-content strong { font-weight: 700; }
.editor-content em { font-style: italic; }
.editor-content u { text-decoration: underline; }
.editor-content s { text-decoration: line-through; color: var(--rinch-color-dimmed); }

.editor-content a {
    color: var(--rinch-color-teal-4);
    text-decoration: underline;
    text-decoration-color: var(--rinch-color-teal-8);
}

.editor-content mark {
    background: var(--rinch-color-teal-9);
    color: var(--rinch-color-teal-2);
    padding: 1px 2px;
    border-radius: 2px;
}

.save-indicator {
    font-size: 12px;
    padding: 2px 10px;
    border-radius: 4px;
    letter-spacing: 0.03em;
    transition: color 0.2s ease;
}

.save-indicator.saving {
    color: var(--rinch-color-teal-5);
}

.save-indicator.saved {
    color: var(--rinch-color-dimmed);
}

.save-indicator.unsaved {
    color: var(--rinch-color-teal-4);
}

@media (max-width: 768px) {
    .editor-content {
        padding: 16px;
    }
    .toolbar {
        padding: 6px 12px;
    }
}
"#;

#[component]
pub fn separator() -> NodeHandle {
    rsx! { div { class: "toolbar-separator" } }
}

/// Toolbar button without active state tracking.
#[component]
pub fn toolbar_button(icon: TablerIcon, tooltip: &str, on_click: impl Fn() + 'static) -> NodeHandle {
    let _ = tooltip;
    rsx! {
        ActionIcon {
            variant: "subtle",
            size: "sm",
            onclick: on_click,
            {render_tabler_icon(__scope, icon, TablerIconStyle::Outline)}
        }
    }
}

/// Toolbar button with active state — plain function (not #[component]) to avoid
/// the Fn closure ownership issue with two non-Copy captured values.
fn fmt_button(
    __scope: &mut RenderScope,
    icon: TablerIcon,
    on_click: impl Fn() + 'static + Copy,
    active: Signal<bool>,
) -> NodeHandle {
    rsx! {
        ActionIcon {
            variant: {move || if active.get() { "light".to_string() } else { "subtle".to_string() }},
            size: "sm",
            onclick: on_click,
            {render_tabler_icon(__scope, icon, TablerIconStyle::Outline)}
        }
    }
}

#[component]
pub fn editor_toolbar() -> NodeHandle {
    // Active state signals for formatting buttons
    let s_bold: Signal<bool> = Signal::new(false);
    let s_italic: Signal<bool> = Signal::new(false);
    let s_underline: Signal<bool> = Signal::new(false);
    let s_strike: Signal<bool> = Signal::new(false);
    let s_h1: Signal<bool> = Signal::new(false);
    let s_h2: Signal<bool> = Signal::new(false);
    let s_h3: Signal<bool> = Signal::new(false);
    let s_bquote: Signal<bool> = Signal::new(false);
    let s_ul: Signal<bool> = Signal::new(false);
    let s_ol: Signal<bool> = Signal::new(false);
    let s_jl: Signal<bool> = Signal::new(false);
    let s_jc: Signal<bool> = Signal::new(false);
    let s_jr: Signal<bool> = Signal::new(false);
    let s_jf: Signal<bool> = Signal::new(false);

    let refresh = move || {
        s_bold.set(query_cmd_state("bold"));
        s_italic.set(query_cmd_state("italic"));
        s_underline.set(query_cmd_state("underline"));
        s_strike.set(query_cmd_state("strikeThrough"));
        let block = query_cmd_value("formatBlock");
        s_h1.set(block.eq_ignore_ascii_case("h1"));
        s_h2.set(block.eq_ignore_ascii_case("h2"));
        s_h3.set(block.eq_ignore_ascii_case("h3"));
        s_bquote.set(block.eq_ignore_ascii_case("blockquote"));
        s_ul.set(query_cmd_state("insertUnorderedList"));
        s_ol.set(query_cmd_state("insertOrderedList"));
        s_jl.set(query_cmd_state("justifyLeft"));
        s_jc.set(query_cmd_state("justifyCenter"));
        s_jr.set(query_cmd_state("justifyRight"));
        s_jf.set(query_cmd_state("justifyFull"));
    };

    // Update toolbar state on selection changes
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        let listener = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
            refresh();
        }) as Box<dyn FnMut(_)>);
        doc.add_event_listener_with_callback("selectionchange", listener.as_ref().unchecked_ref()).ok();
        listener.forget();
    }

    rsx! {
        div { class: "toolbar",
            // Inline formatting
            {fmt_button(__scope, TablerIcon::Bold,
                move || { exec_cmd("bold"); refresh(); }, s_bold)}
            {fmt_button(__scope, TablerIcon::Italic,
                move || { exec_cmd("italic"); refresh(); }, s_italic)}
            {fmt_button(__scope, TablerIcon::Underline,
                move || { exec_cmd("underline"); refresh(); }, s_underline)}
            {fmt_button(__scope, TablerIcon::Strikethrough,
                move || { exec_cmd("strikeThrough"); refresh(); }, s_strike)}
            {toolbar_button(__scope, TablerIcon::Code, "Code", move || {
                // Wrap selection in <code> using insertHTML
                if let Some(win) = web_sys::window() {
                    if let Some(doc) = win.document() {
                        if let Ok(Some(sel)) = doc.get_selection() {
                            if sel.range_count() > 0 {
                                if let Ok(range) = sel.get_range_at(0) {
                                    let text = range.to_string();
                                    let _ = range.delete_contents();
                                    exec_cmd_val("insertHTML", &format!("<code>{}</code>", text.as_string().unwrap_or_default()));
                                }
                            }
                        }
                    }
                }
            })}

            {separator(__scope)}

            // Block types
            {fmt_button(__scope, TablerIcon::H1,
                move || { exec_cmd_val("formatBlock", "h1"); refresh(); }, s_h1)}
            {fmt_button(__scope, TablerIcon::H2,
                move || { exec_cmd_val("formatBlock", "h2"); refresh(); }, s_h2)}
            {fmt_button(__scope, TablerIcon::H3,
                move || { exec_cmd_val("formatBlock", "h3"); refresh(); }, s_h3)}

            {separator(__scope)}

            // Block elements
            {fmt_button(__scope, TablerIcon::Blockquote,
                move || { exec_cmd_val("formatBlock", "blockquote"); refresh(); }, s_bquote)}
            {fmt_button(__scope, TablerIcon::List,
                move || { exec_cmd("insertUnorderedList"); refresh(); }, s_ul)}
            {fmt_button(__scope, TablerIcon::ListNumbers,
                move || { exec_cmd("insertOrderedList"); refresh(); }, s_ol)}

            {separator(__scope)}

            // Indent / Outdent
            {toolbar_button(__scope, TablerIcon::IndentIncrease, "Indent", move || exec_cmd("indent"))}
            {toolbar_button(__scope, TablerIcon::IndentDecrease, "Outdent", move || exec_cmd("outdent"))}

            {separator(__scope)}

            // Text alignment
            {fmt_button(__scope, TablerIcon::AlignLeft,
                move || { exec_cmd("justifyLeft"); refresh(); }, s_jl)}
            {fmt_button(__scope, TablerIcon::AlignCenter,
                move || { exec_cmd("justifyCenter"); refresh(); }, s_jc)}
            {fmt_button(__scope, TablerIcon::AlignRight,
                move || { exec_cmd("justifyRight"); refresh(); }, s_jr)}
            {fmt_button(__scope, TablerIcon::AlignJustified,
                move || { exec_cmd("justifyFull"); refresh(); }, s_jf)}

            {separator(__scope)}

            // Undo / Redo
            {toolbar_button(__scope, TablerIcon::ArrowBackUp, "Undo (Ctrl+Z)", move || exec_cmd("undo"))}
            {toolbar_button(__scope, TablerIcon::ArrowForwardUp, "Redo (Ctrl+Shift+Z)", move || exec_cmd("redo"))}
        }
    }
}

/// Simple markdown to HTML converter for loading content.
pub fn markdown_to_html(md: &str) -> String {
    let mut html = String::new();
    let mut in_list = false;
    let mut list_type = "";
    let mut pending_align: Option<&str> = None;

    for line in md.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if in_list {
                html.push_str(&format!("</{}>", list_type));
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
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<h3{}>{}</h3>", style_attr, inline_md(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<h2{}>{}</h2>", style_attr, inline_md(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<h1{}>{}</h1>", style_attr, inline_md(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("> ") {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<blockquote><p{}>{}</p></blockquote>", style_attr, inline_md(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            if !in_list || list_type != "ul" {
                if in_list { html.push_str(&format!("</{}>", list_type)); }
                html.push_str("<ul>");
                in_list = true;
                list_type = "ul";
            }
            html.push_str(&format!("<li>{}</li>", inline_md(rest)));
        } else if trimmed.len() > 2 && trimmed.as_bytes()[0].is_ascii_digit() && trimmed.contains(". ") {
            let rest = &trimmed[trimmed.find(". ").unwrap() + 2..];
            if !in_list || list_type != "ol" {
                if in_list { html.push_str(&format!("</{}>", list_type)); }
                html.push_str("<ol>");
                in_list = true;
                list_type = "ol";
            }
            html.push_str(&format!("<li>{}</li>", inline_md(rest)));
        } else if trimmed == "---" || trimmed == "***" {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str("<hr>");
        } else {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<p{}>{}</p>", style_attr, inline_md(trimmed)));
        }
    }

    if in_list {
        html.push_str(&format!("</{}>", list_type));
    }

    if html.is_empty() {
        html.push_str("<p></p>");
    }

    html
}

/// Process inline markdown: bold, italic, code, strikethrough.
pub fn inline_md(text: &str) -> String {
    let mut result = text.to_string();
    // Bold: **text** or __text__
    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let inner = &result[start + 2..start + 2 + end].to_string();
            result = format!(
                "{}<strong>{}</strong>{}",
                &result[..start],
                inner,
                &result[start + 2 + end + 2..]
            );
        } else {
            break;
        }
    }
    // Italic: *text* or _text_
    while let Some(start) = result.find('*') {
        if let Some(end) = result[start + 1..].find('*') {
            let inner = &result[start + 1..start + 1 + end].to_string();
            result = format!(
                "{}<em>{}</em>{}",
                &result[..start],
                inner,
                &result[start + 1 + end + 1..]
            );
        } else {
            break;
        }
    }
    // Code: `text`
    while let Some(start) = result.find('`') {
        if let Some(end) = result[start + 1..].find('`') {
            let inner = &result[start + 1..start + 1 + end].to_string();
            result = format!(
                "{}<code>{}</code>{}",
                &result[..start],
                inner,
                &result[start + 1 + end + 1..]
            );
        } else {
            break;
        }
    }
    // Strikethrough: ~~text~~
    while let Some(start) = result.find("~~") {
        if let Some(end) = result[start + 2..].find("~~") {
            let inner = &result[start + 2..start + 2 + end].to_string();
            result = format!(
                "{}<s>{}</s>{}",
                &result[..start],
                inner,
                &result[start + 2 + end + 2..]
            );
        } else {
            break;
        }
    }
    result
}

/// Extract text-align value from a tag's attributes string.
/// Checks both `style="text-align: center"` and `align="center"` (legacy).
fn extract_text_align(tag_attrs: &str) -> Option<&'static str> {
    // Check style attribute
    if let Some(style_start) = tag_attrs.find("text-align:") {
        let rest = &tag_attrs[style_start + 11..];
        let rest = rest.trim_start();
        if rest.starts_with("center") {
            return Some("center");
        } else if rest.starts_with("right") {
            return Some("right");
        } else if rest.starts_with("justify") {
            return Some("justify");
        }
    }
    // Check legacy align attribute: align="center"
    if let Some(align_start) = tag_attrs.find("align=\"") {
        let rest = &tag_attrs[align_start + 7..];
        if rest.starts_with("center") {
            return Some("center");
        } else if rest.starts_with("right") {
            return Some("right");
        } else if rest.starts_with("justify") {
            return Some("justify");
        }
    }
    None
}

/// Simple HTML to markdown converter for saving content.
pub fn html_to_markdown(html: &str) -> String {
    let mut md = String::new();
    let mut chars = html.chars().peekable();
    let mut in_tag = false;
    let mut tag_name = String::new();
    let mut is_closing = false;
    let mut text_buffer = String::new();
    let mut list_stack: Vec<&str> = Vec::new();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Flush text
            if !text_buffer.is_empty() {
                md.push_str(&text_buffer);
                text_buffer.clear();
            }
            in_tag = true;
            tag_name.clear();
            is_closing = false;
            if chars.peek() == Some(&'/') {
                is_closing = true;
                chars.next();
            }
        } else if ch == '>' && in_tag {
            in_tag = false;
            let tag = tag_name.split_whitespace().next().unwrap_or("").to_lowercase();

            if !is_closing {
                // Check for text-align in tag attributes
                if let Some(align) = extract_text_align(&tag_name) {
                    md.push_str(&format!("{{align:{}}}\n", align));
                }

                match tag.as_str() {
                    "h1" => md.push_str("# "),
                    "h2" => md.push_str("## "),
                    "h3" => md.push_str("### "),
                    "h4" => md.push_str("#### "),
                    "h5" => md.push_str("##### "),
                    "h6" => md.push_str("###### "),
                    "strong" | "b" => md.push_str("**"),
                    "em" | "i" => md.push('*'),
                    "code" => md.push('`'),
                    "s" | "del" => md.push_str("~~"),
                    "blockquote" => md.push_str("> "),
                    "li" => {
                        if list_stack.last() == Some(&"ol") {
                            md.push_str("1. ");
                        } else {
                            md.push_str("- ");
                        }
                    }
                    "ul" => list_stack.push("ul"),
                    "ol" => list_stack.push("ol"),
                    "hr" => md.push_str("---\n"),
                    "br" => md.push('\n'),
                    _ => {}
                }
            } else {
                match tag.as_str() {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "div" | "blockquote" | "li" => {
                        md.push('\n');
                    }
                    "strong" | "b" => md.push_str("**"),
                    "em" | "i" => md.push('*'),
                    "code" => md.push('`'),
                    "s" | "del" => md.push_str("~~"),
                    "ul" | "ol" => {
                        list_stack.pop();
                        md.push('\n');
                    }
                    _ => {}
                }
            }
        } else if in_tag {
            tag_name.push(ch);
        } else {
            text_buffer.push(ch);
        }
    }

    if !text_buffer.is_empty() {
        md.push_str(&text_buffer);
    }

    // Clean up extra newlines
    while md.contains("\n\n\n") {
        md = md.replace("\n\n\n", "\n\n");
    }
    md.trim().to_string()
}
