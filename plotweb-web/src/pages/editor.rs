use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{Book, Chapter, UpdateChapterRequest};

use crate::api;
use crate::fonts;
use crate::store::{AppStore, Route};

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// Call document.execCommand(command, false, value) via JS.
    #[wasm_bindgen(js_namespace = document, js_name = execCommand, catch)]
    fn exec_command_js(command: &str, show_ui: bool, value: &str) -> Result<bool, JsValue>;
}

/// Execute a browser execCommand on the document (for contenteditable formatting).
fn exec_cmd(command: &str) {
    exec_command_js(command, false, "").ok();
}

/// Execute a browser execCommand with a value argument.
fn exec_cmd_val(command: &str, value: &str) {
    exec_command_js(command, false, value).ok();
}

/// Editor CSS — focused writing environment with semantic colors.
const EDITOR_CSS: &str = r#"
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
}

.toolbar-separator {
    width: 1px;
    height: 20px;
    background: var(--rinch-color-border);
    margin: 0 6px;
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
"#;

#[component]
fn separator() -> NodeHandle {
    rsx! { div { class: "toolbar-separator" } }
}

#[component]
fn toolbar_button(icon: TablerIcon, tooltip: &str, on_click: impl Fn() + 'static) -> NodeHandle {
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

#[component]
fn editor_toolbar() -> NodeHandle {
    rsx! {
        div { class: "toolbar",
            // Inline formatting
            {toolbar_button(__scope, TablerIcon::Bold, "Bold (Ctrl+B)", move || exec_cmd("bold"))}
            {toolbar_button(__scope, TablerIcon::Italic, "Italic (Ctrl+I)", move || exec_cmd("italic"))}
            {toolbar_button(__scope, TablerIcon::Underline, "Underline (Ctrl+U)", move || exec_cmd("underline"))}
            {toolbar_button(__scope, TablerIcon::Strikethrough, "Strikethrough", move || exec_cmd("strikeThrough"))}
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
            {toolbar_button(__scope, TablerIcon::H1, "Heading 1", move || exec_cmd_val("formatBlock", "h1"))}
            {toolbar_button(__scope, TablerIcon::H2, "Heading 2", move || exec_cmd_val("formatBlock", "h2"))}
            {toolbar_button(__scope, TablerIcon::H3, "Heading 3", move || exec_cmd_val("formatBlock", "h3"))}

            {separator(__scope)}

            // Block elements
            {toolbar_button(__scope, TablerIcon::Blockquote, "Blockquote", move || exec_cmd_val("formatBlock", "blockquote"))}
            {toolbar_button(__scope, TablerIcon::List, "Bullet List", move || exec_cmd("insertUnorderedList"))}
            {toolbar_button(__scope, TablerIcon::ListNumbers, "Numbered List", move || exec_cmd("insertOrderedList"))}

            {separator(__scope)}

            // Indent / Outdent
            {toolbar_button(__scope, TablerIcon::IndentIncrease, "Indent", move || exec_cmd("indent"))}
            {toolbar_button(__scope, TablerIcon::IndentDecrease, "Outdent", move || exec_cmd("outdent"))}

            {separator(__scope)}

            // Undo / Redo
            {toolbar_button(__scope, TablerIcon::ArrowBackUp, "Undo (Ctrl+Z)", move || exec_cmd("undo"))}
            {toolbar_button(__scope, TablerIcon::ArrowForwardUp, "Redo (Ctrl+Shift+Z)", move || exec_cmd("redo"))}
        }
    }
}

#[component]
pub fn editor_page(book_id: String, chapter_id: String) -> NodeHandle {
    let store = use_store::<AppStore>();
    let chapter_title = Signal::new(String::new());
    let save_status = Signal::new("saved"); // "saved", "saving", "unsaved"
    let loaded = Signal::new(false);
    let editor_el_id = format!("editor-{}", chapter_id);

    // Fetch book metadata (for font settings)
    let bid_book = book_id.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if store.current_book.get().as_ref().map(|b| &b.id) != Some(&bid_book) {
            if let Ok(book) = api::get::<Book>(&format!("/api/books/{}", bid_book)).await {
                let fs = book.font_settings.clone().unwrap_or_default();
                fonts::load_book_fonts(&fs);
                store.current_book.set(Some(book));
            }
        } else if let Some(book) = store.current_book.get().as_ref() {
            let fs = book.font_settings.clone().unwrap_or_default();
            fonts::load_book_fonts(&fs);
        }
    });

    // Fetch chapter content
    let bid = book_id.clone();
    let cid = chapter_id.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(chapter) = api::get::<Chapter>(
            &format!("/api/books/{}/chapters/{}", bid, cid),
        ).await {
            chapter_title.set(chapter.title.clone());
            // Set editor content — the contenteditable div will have this as initial content
            // We store it so it can be placed into the editor
            loaded.set(true);

            // If there's existing content, inject it into the contenteditable div
            if !chapter.content.is_empty() {
                // Use a small delay to ensure the DOM is ready
                let content = chapter.content.clone();
                let load_el_id = cid.clone();
                let window = web_sys::window().unwrap();
                let closure = wasm_bindgen::closure::Closure::once(move || {
                    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                        if let Ok(Some(el)) = doc.query_selector(&format!("#editor-{}", load_el_id)) {
                            // Convert markdown to simple HTML for display
                            // The CE API will handle the editing
                            let html = markdown_to_html(&content);
                            el.set_inner_html(&html);
                        }
                    }
                });
                window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    100,
                ).ok();
                closure.forget();
            }
        }
    });

    // Auto-save: set up a mechanism for saving
    let bid_save = book_id.clone();
    let cid_save = chapter_id.clone();
    let save_content = move || {
        let bid = bid_save.clone();
        let cid = cid_save.clone();
        save_status.set("saving");
        wasm_bindgen_futures::spawn_local(async move {
            // Get content from contenteditable div — bail if the editor isn't mounted
            let content = match web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(&format!("#editor-{}", cid)).ok().flatten())
            {
                Some(el) => el.inner_html(),
                None => {
                    // Editor not mounted (navigated away) — don't save empty content
                    save_status.set("saved");
                    return;
                }
            };

            // Convert HTML back to markdown for storage
            let markdown = html_to_markdown(&content);

            let req = UpdateChapterRequest {
                title: None,
                content: Some(markdown),
            };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", bid, cid),
                &req,
            ).await.is_ok() {
                save_status.set("saved");
            } else {
                save_status.set("unsaved");
            }
        });
    };

    // Set up auto-save timer and Ctrl+S handler
    {
        let save = save_content.clone();
        let window = web_sys::window().unwrap();

        // Ctrl+S handler
        let save_for_key = save.clone();
        let keydown = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if (event.ctrl_key() || event.meta_key()) && event.key() == "s" {
                event.prevent_default();
                save_for_key();
            }
        }) as Box<dyn FnMut(_)>);
        window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref()).ok();
        keydown.forget();

        // Auto-save on input with debounce (3 seconds)
        let save_for_input = save;
        let timer_id = Signal::new(0_i32);
        let input_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
            // Only react to input events from inside the editor's contenteditable div.
            // This listener is attached to the document and never removed, so without
            // this guard, typing in *any* input (e.g. font search on the book page)
            // would trigger a save that reads empty content and wipes the chapter.
            let dominated_by_editor = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .and_then(|el| el.closest(".editor-content[contenteditable]").ok().flatten())
                .is_some();
            if !dominated_by_editor {
                return;
            }

            save_status.set("unsaved");
            // Clear previous timer
            let prev = timer_id.get();
            if prev != 0 {
                web_sys::window().unwrap().clear_timeout_with_handle(prev);
            }
            // Set new timer
            let save_fn = save_for_input.clone();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                save_fn();
            });
            let id = web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    3000,
                ).unwrap_or(0);
            closure.forget();
            timer_id.set(id);
        }) as Box<dyn FnMut(_)>);

        // Attach to document — will capture input events from contenteditable
        let doc = window.document().unwrap();
        doc.add_event_listener_with_callback("input", input_closure.as_ref().unchecked_ref()).ok();
        input_closure.forget();
    }

    let go_back = {
        let bid = book_id.clone();
        move || {
            store.current_route.set(Route::Book(bid.clone()));
        }
    };

    rsx! {
        Fragment {
            style { {EDITOR_CSS} }
            style {
                {move || {
                    let fs = store.current_book.get()
                        .and_then(|b| b.font_settings.clone())
                        .unwrap_or_default();

                    let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
                    let h2 = fs.h2.as_deref().unwrap_or("Macondo Swash Caps");
                    let h3 = fs.h3.as_deref().unwrap_or("Macondo Swash Caps");
                    let body = fs.body.as_deref().unwrap_or("Andada Pro");
                    let quote = fs.quote.as_deref().unwrap_or("inherit");
                    let code = fs.code.as_deref().unwrap_or("monospace");

                    format!(
                        ".editor-content {{ font-family: '{body}', serif; }}
                         .editor-content h1 {{ font-family: '{h1}', cursive; }}
                         .editor-content h2 {{ font-family: '{h2}', cursive; }}
                         .editor-content h3, .editor-content h4,
                         .editor-content h5, .editor-content h6 {{ font-family: '{h3}', cursive; }}
                         .editor-content blockquote {{ font-family: '{quote}', serif; }}
                         .editor-content code, .editor-content pre {{ font-family: '{code}', monospace; }}"
                    )
                }}
            }
            div { class: "editor-layout",
                // Top bar with chapter title and save status
                div { class: "editor-topbar",
                    div { class: "editor-topbar-left",
                        ActionIcon {
                            variant: "subtle",
                            onclick: go_back,
                            {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                        }
                        Text { weight: "600", {|| chapter_title.get()} }
                    }
                    div {
                        class: {|| format!("save-indicator {}", save_status.get())},
                        {|| match save_status.get() {
                            "saving" => "Saving...".to_string(),
                            "saved" => "Saved".to_string(),
                            _ => "Unsaved".to_string(),
                        }}
                    }
                }

                // Formatting toolbar
                {editor_toolbar(__scope)}

                // Editor surface
                div { class: "editor-scroll",
                    div {
                        contenteditable: "true",
                        class: "editor-content",
                        id: editor_el_id,
                        p { "Start writing..." }
                    }
                }
            }
        }
    }
}

/// Simple markdown to HTML converter for loading content.
fn markdown_to_html(md: &str) -> String {
    let mut html = String::new();
    let mut in_list = false;
    let mut list_type = "";

    for line in md.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if in_list {
                html.push_str(&format!("</{}>", list_type));
                in_list = false;
            }
            continue;
        }

        // Headings
        if let Some(rest) = trimmed.strip_prefix("### ") {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<h3>{}</h3>", inline_md(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<h2>{}</h2>", inline_md(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<h1>{}</h1>", inline_md(rest)));
        } else if let Some(rest) = trimmed.strip_prefix("> ") {
            if in_list { html.push_str(&format!("</{}>", list_type)); in_list = false; }
            html.push_str(&format!("<blockquote><p>{}</p></blockquote>", inline_md(rest)));
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
            html.push_str(&format!("<p>{}</p>", inline_md(trimmed)));
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
fn inline_md(text: &str) -> String {
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

/// Simple HTML to markdown converter for saving content.
fn html_to_markdown(html: &str) -> String {
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
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "blockquote" | "li" => {
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
