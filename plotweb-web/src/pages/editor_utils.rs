use std::rc::Rc;

use rinch::prelude::*;
use rinch_core::Signal;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use crate::rinch_backend::EditorHandle;
use rinch_editor_core::serialize::DocNode;

use wasm_bindgen::prelude::*;

// ── Model-first editor (rinch-editor-view) content bridge ──────────────────
//
// Chapters/notes are stored as an **opaque String** the frontend owns. New saves
// are `DocNode` JSON (the editor's durable wire shape); legacy content is still
// chapter Markdown / note HTML. Loads are legacy-tolerant: try DocNode JSON first,
// fall back to the pre-8a HTML path. A later card bulk-migrates legacy content.

/// Load stored chapter content into `handle`. Tries `DocNode` JSON (new format),
/// falling back to the legacy Markdown → HTML path (`markdown_to_html`).
pub fn load_chapter_content(handle: &EditorHandle, content: &str) {
    if load_docnode(handle, content) {
        return;
    }
    // Legacy: chapters were stored as Markdown. `markdown_to_html` yields `<p></p>`
    // for empty input, which `load_html` turns into a single empty paragraph.
    handle.load_html(&markdown_to_html(content));
}

/// Load stored note content into `handle`. Tries `DocNode` JSON (new format),
/// falling back to the legacy raw-HTML path (notes were already HTML).
pub fn load_note_content(handle: &EditorHandle, content: &str) {
    if load_docnode(handle, content) {
        return;
    }
    if content.trim().is_empty() {
        handle.load_html("<p></p>");
    } else {
        handle.load_html(content);
    }
}

/// Try to parse `content` as `DocNode` JSON and load it via the schema. Returns
/// `false` (leaving the editor untouched) when `content` isn't DocNode JSON, so
/// callers can fall back to the legacy HTML path.
fn load_docnode(handle: &EditorHandle, content: &str) -> bool {
    let trimmed = content.trim_start();
    if !trimmed.starts_with('{') {
        return false;
    }
    let Ok(docnode) = serde_json::from_str::<DocNode>(trimmed) else {
        return false;
    };
    let state = handle.state();
    match state.schema().node_from_doc(&docnode) {
        Ok(node) => {
            handle.load_doc(node);
            true
        }
        Err(_) => false,
    }
}

/// Serialize the editor's current document to `DocNode` JSON for saving, or `None`
/// if the document can't be serialized (never expected for a live editor).
pub fn editor_content_json(handle: &EditorHandle) -> Option<String> {
    match handle.doc().to_doc() {
        Ok(docnode) => serde_json::to_string(&docnode).ok(),
        Err(_) => None,
    }
}

/// Render stored chapter/note content to display HTML for read-only views (the
/// reader, the history preview).
///
/// New content is `DocNode` JSON (the editor's durable save shape): deserialize
/// it against the starter-kit schema and project it with `node_to_html` — the
/// same HTML the editor renders. Legacy content (chapter Markdown / note HTML)
/// falls back to `markdown_to_html`. Callers still pass the result through
/// [`sanitize_html`] before injecting it into the DOM.
pub fn content_to_display_html(content: &str) -> String {
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(docnode) = serde_json::from_str::<DocNode>(trimmed) {
            let schema = rinch_editor_core::Schema::starter_kit();
            if let Ok(node) = schema.node_from_doc(&docnode) {
                return rinch_editor_core::serialize::node_to_html(&node);
            }
        }
    }
    markdown_to_html(content)
}

/// Count words in the editor's current document (whitespace-separated words across
/// all text nodes).
pub fn editor_word_count(handle: &EditorHandle) -> u64 {
    match handle.doc().to_doc() {
        Ok(docnode) => count_docnode_words(&docnode),
        Err(_) => 0,
    }
}

fn count_docnode_words(node: &DocNode) -> u64 {
    let mut count = node
        .text
        .as_deref()
        .map(|t| t.split_whitespace().count() as u64)
        .unwrap_or(0);
    for child in &node.content {
        count += count_docnode_words(child);
    }
    count
}

/// The heading level (1-6) the cursor is in, or `None` when not in a heading —
/// drives the H1/H2/H3 toolbar active states.
fn current_heading_level(handle: &EditorHandle) -> Option<i64> {
    if handle.current_block_type().as_deref() != Some("heading") {
        return None;
    }
    let state = handle.state();
    let resolved = state.doc.resolve(state.selection.head()).ok()?;
    resolved.parent().attrs().get_int("level")
}

/// Invoke `action` with the element matching `selector` as soon as it exists in
/// the DOM, polling on animation frames rather than guessing a fixed timeout.
///
/// This replaces the old `setTimeout(…, 100ms)` hacks that blindly waited for
/// rinch to (re)render a node before injecting content into it: too short and
/// the injection silently no-ops (lost content / stuck "read-only" editor); too
/// long and chapter switches feel laggy. Polling on rAF injects on the first
/// frame the node is present — deterministic regardless of render timing.
///
/// Capped at ~1s of frames so a selector that never matches can't spin forever.
/// Implemented as chained one-shot closures (each `forget()`-ed, matching the
/// crate's existing pattern) rather than a single self-rescheduling closure,
/// which would have to drop itself mid-call — undefined behavior in wasm.
pub fn with_element_when_ready(
    selector: String,
    action: impl FnOnce(&web_sys::Element) + 'static,
) {
    fn attempt(selector: String, frames_left: u32, action: Box<dyn FnOnce(&web_sys::Element)>) {
        if let Some(el) = crate::platform::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector(&selector).ok().flatten())
        {
            action(&el);
            return;
        }
        if frames_left == 0 {
            return;
        }
        let closure = wasm_bindgen::closure::Closure::once(move || {
            attempt(selector, frames_left - 1, action);
        });
        if let Some(w) = crate::platform::window() {
            w.request_animation_frame(closure.as_ref().unchecked_ref()).ok();
        }
        closure.forget();
    }
    // ~1s at 60fps; the target node normally exists within a frame or two.
    attempt(selector, 60, Box::new(action));
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

/* The wrapper we own: page layout only. Typography lives on the editor's own
   container below — see the note there. */
.editor-content {
    min-height: 100%;
    max-width: 720px;
    margin: 0 auto;
    padding: 48px 48px;
    outline: none;
    cursor: text;
}

/* ── Prose inside the editor ─────────────────────────────────────────────────
   rinch-editor-view injects a batteries-included stylesheet scoped to
   `[data-pm-editor]` (system sans, GitHub-ish palette, its own margins). It
   styles that container *directly*, so our typography cannot simply cascade in
   from `.editor-content` — inheritance stops at its container — and its
   `p`/heading rules tie ours on specificity and win on source order (it injects
   later, on first mount).

   rinch's documented contract is "override with higher-specificity styles" (a
   full opt-out isn't viable: its stylesheet is load-bearing for tables, which
   rinch-dom lays out with flexbox). So these rules go through the wrapper
   **ids** — (1,1,0) — which also beats its dark rules
   (`[data-pm-editor][data-pm-theme="dark"]`, (0,2,0)). Anything we don't
   restate keeps rinch's default, which is what we want.

   `font-family: inherit` pulls the app font — and the per-book Typography
   setting, which targets `.editor-content` — down into the prose. */
#editor-main [data-pm-editor],
#note-editor-main [data-pm-editor] {
    font-family: inherit;
    font-size: 16px;
    line-height: 1.8;
    color: var(--rinch-color-text);
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;

    /* rinch's default container is a bordered "card" (white/#0d1117 background,
       1px border, 10px radius, its own padding) — right for a standalone editor,
       wrong here: our wrapper already supplies the page's surface, width and
       padding, so the writing area must be seamless.

       Only the chrome is reset. rinch's container also carries `white-space:
       pre-wrap` / `overflow-wrap` (load-bearing: without them the rendered text
       stops matching the model 1:1 and the caret's DOM-offset → model-byte
       mapping breaks) and `position: relative` / `z-index` (the caret and
       selection overlays are absolutely positioned against it). Those must be
       left alone. */
    background: transparent;
    border: none;
    border-radius: 0;
    padding: 0;
}

#editor-main [data-pm-editor] p,
#note-editor-main [data-pm-editor] p { margin: 0 0 8px 0; }
#editor-main [data-pm-editor] h1,
#note-editor-main [data-pm-editor] h1 { font-size: 2em; font-weight: 700; margin: 32px 0 12px 0; color: var(--rinch-color-text); }
#editor-main [data-pm-editor] h2,
#note-editor-main [data-pm-editor] h2 { font-size: 1.5em; font-weight: 700; margin: 28px 0 10px 0; color: var(--rinch-color-text); }
#editor-main [data-pm-editor] h3,
#note-editor-main [data-pm-editor] h3 { font-size: 1.25em; font-weight: 600; margin: 24px 0 8px 0; color: var(--rinch-color-text); }
#editor-main [data-pm-editor] h4,
#note-editor-main [data-pm-editor] h4 { font-size: 1.1em; font-weight: 600; margin: 16px 0 6px 0; color: var(--rinch-color-text); }
#editor-main [data-pm-editor] h5,
#note-editor-main [data-pm-editor] h5 { font-size: 1em; font-weight: 600; margin: 12px 0 4px 0; color: var(--rinch-color-dimmed); }
#editor-main [data-pm-editor] h6,
#note-editor-main [data-pm-editor] h6 { font-size: 0.9em; font-weight: 600; margin: 12px 0 4px 0; color: var(--rinch-color-dimmed); }

#editor-main [data-pm-editor] img,
#note-editor-main [data-pm-editor] img {
    max-width: 100%;
    height: auto;
    border-radius: var(--rinch-radius-sm);
    margin: 16px 0;
    display: block;
}

#editor-main [data-pm-editor] blockquote,
#note-editor-main [data-pm-editor] blockquote {
    border-left: 3px solid var(--rinch-color-teal-8);
    padding-left: 16px;
    margin: 16px 0;
    color: var(--rinch-color-dimmed);
}

#editor-main [data-pm-editor] pre,
#note-editor-main [data-pm-editor] pre {
    background: var(--pw-color-deep);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    padding: 12px 16px;
    margin: 16px 0;
    font-family: monospace;
    font-size: 14px;
    overflow-x: auto;
}

#editor-main [data-pm-editor] code,
#note-editor-main [data-pm-editor] code {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 0.9em;
    color: var(--rinch-color-teal-4);
}

#editor-main [data-pm-editor] pre code,
#note-editor-main [data-pm-editor] pre code {
    background: none;
    border: none;
    padding: 0;
    border-radius: 0;
    color: inherit;
}

#editor-main [data-pm-editor] ul,
#note-editor-main [data-pm-editor] ul,
#editor-main [data-pm-editor] ol,
#note-editor-main [data-pm-editor] ol {
    margin: 8px 0;
    padding-left: 24px;
}

#editor-main [data-pm-editor] li,
#note-editor-main [data-pm-editor] li { margin: 4px 0; }

#editor-main [data-pm-editor] hr,
#note-editor-main [data-pm-editor] hr {
    border: none;
    border-top: 1px solid var(--rinch-color-border);
    margin: 24px 0;
}

#editor-main [data-pm-editor] strong,
#note-editor-main [data-pm-editor] strong { font-weight: 700; }
#editor-main [data-pm-editor] em,
#note-editor-main [data-pm-editor] em { font-style: italic; }
#editor-main [data-pm-editor] u,
#note-editor-main [data-pm-editor] u { text-decoration: underline; }
#editor-main [data-pm-editor] s,
#note-editor-main [data-pm-editor] s { text-decoration: line-through; color: var(--rinch-color-dimmed); }

#editor-main [data-pm-editor] a,
#note-editor-main [data-pm-editor] a {
    color: var(--rinch-color-teal-4);
    text-decoration: underline;
    text-decoration-color: var(--rinch-color-teal-8);
}

#editor-main [data-pm-editor] mark,
#note-editor-main [data-pm-editor] mark {
    background: var(--rinch-color-teal-9);
    color: var(--rinch-color-teal-2);
    padding: 1px 2px;
    border-radius: 2px;
}

.editor-word-count {
    font-size: 12px;
    color: var(--rinch-color-dimmed);
    padding: 2px 6px;
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

.save-indicator.error {
    color: var(--rinch-color-red-6, #e03131);
    font-weight: 600;
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
    on_click: impl Fn() + 'static + Clone,
    active: Signal<bool>,
) -> NodeHandle {
    rsx! {
        ActionIcon {
            variant: {move || if active.get() { "light".to_string() } else { "subtle".to_string() }},
            size: "sm",
            onclick: on_click.clone(),
            {render_tabler_icon(__scope, icon, TablerIconStyle::Outline)}
        }
    }
}

/// The editor toolbar, driving `handle` (a `rinch-editor-view` [`EditorHandle`])
/// through `handle.command(...)` — the same model-first API desktop uses. `on_edit`
/// is called after any content-mutating action so the caller can schedule an
/// autosave. Text alignment is intentionally omitted (no editor command yet — a
/// separate upstream card adds it).
///
/// A plain function (not `#[component]`) so it can take the non-`Copy` `EditorHandle`
/// and the `on_edit` closure directly.
pub fn editor_toolbar(
    __scope: &mut RenderScope,
    handle: EditorHandle,
    book_id: String,
    on_edit: impl Fn() + 'static + Copy,
) -> NodeHandle {
    // Active-state signals for formatting buttons (toolbar "on" highlight).
    let s_bold: Signal<bool> = Signal::new(false);
    let s_italic: Signal<bool> = Signal::new(false);
    let s_underline: Signal<bool> = Signal::new(false);
    let s_strike: Signal<bool> = Signal::new(false);
    let s_code: Signal<bool> = Signal::new(false);
    let s_h1: Signal<bool> = Signal::new(false);
    let s_h2: Signal<bool> = Signal::new(false);
    let s_h3: Signal<bool> = Signal::new(false);
    let s_bquote: Signal<bool> = Signal::new(false);
    let s_ul: Signal<bool> = Signal::new(false);
    let s_ol: Signal<bool> = Signal::new(false);

    // Recompute active states from the editor model. Shared (Rc) so every button
    // closure and the document listeners can call it.
    let refresh: Rc<dyn Fn()> = {
        let h = handle.clone();
        Rc::new(move || {
            s_bold.set(h.is_mark_active("bold"));
            s_italic.set(h.is_mark_active("italic"));
            s_underline.set(h.is_mark_active("underline"));
            s_strike.set(h.is_mark_active("strike"));
            s_code.set(h.is_mark_active("code"));
            let level = current_heading_level(&h);
            s_h1.set(level == Some(1));
            s_h2.set(level == Some(2));
            s_h3.set(level == Some(3));
            s_bquote.set(h.in_node_type("blockquote"));
            s_ul.set(h.in_node_type("bullet_list"));
            s_ol.set(h.in_node_type("ordered_list"));
        })
    };

    // Keep the highlight roughly in sync with the caret/selection. The model editor
    // consumes keydown at capture phase (no `contenteditable`, no native
    // `execCommand`), but keyup / mouseup / selectionchange still bubble to the
    // document, so refreshing on those keeps the toolbar state fresh.
    if let Some(doc) = crate::platform::window().and_then(|w| w.document()) {
        let r = refresh.clone();
        let listener = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
            r();
        }) as Box<dyn FnMut(_)>);
        for ev in ["selectionchange", "keyup", "mouseup"] {
            doc.add_event_listener_with_callback(ev, listener.as_ref().unchecked_ref()).ok();
        }
        let cleanup_doc = doc.clone();
        __scope.on_cleanup(move || {
            for ev in ["selectionchange", "keyup", "mouseup"] {
                cleanup_doc
                    .remove_event_listener_with_callback(ev, listener.as_ref().unchecked_ref())
                    .ok();
            }
            drop(listener);
        });
    }

    // Build a mark/block button closure: run `cmd`, refresh actives, notify on_edit.
    macro_rules! cmd_click {
        ($cmd:expr) => {{
            let h = handle.clone();
            let r = refresh.clone();
            move || {
                h.command($cmd);
                r();
                on_edit();
            }
        }};
    }

    rsx! {
        div { class: "toolbar",
            // Inline formatting marks
            {fmt_button(__scope, TablerIcon::Bold, cmd_click!("toggleBold"), s_bold)}
            {fmt_button(__scope, TablerIcon::Italic, cmd_click!("toggleItalic"), s_italic)}
            {fmt_button(__scope, TablerIcon::Underline, cmd_click!("toggleUnderline"), s_underline)}
            {fmt_button(__scope, TablerIcon::Strikethrough, cmd_click!("toggleStrike"), s_strike)}
            {fmt_button(__scope, TablerIcon::Code, cmd_click!("toggleCode"), s_code)}

            {separator(__scope)}

            // Headings
            {fmt_button(__scope, TablerIcon::H1, cmd_click!("setHeading1"), s_h1)}
            {fmt_button(__scope, TablerIcon::H2, cmd_click!("setHeading2"), s_h2)}
            {fmt_button(__scope, TablerIcon::H3, cmd_click!("setHeading3"), s_h3)}

            {separator(__scope)}

            // Block elements
            {fmt_button(__scope, TablerIcon::Blockquote, cmd_click!("wrapInBlockquote"), s_bquote)}
            {fmt_button(__scope, TablerIcon::List, cmd_click!("toggleBulletList"), s_ul)}
            {fmt_button(__scope, TablerIcon::ListNumbers, cmd_click!("toggleOrderedList"), s_ol)}

            {separator(__scope)}

            // Indent / Outdent (no active state)
            {toolbar_button(__scope, TablerIcon::IndentIncrease, "Indent", cmd_click!("indent"))}
            {toolbar_button(__scope, TablerIcon::IndentDecrease, "Outdent", cmd_click!("outdent"))}

            {separator(__scope)}

            // TODO(8a): link button — no name-only link command; needs a Transaction
            // adding the "link" mark with an href attr via handle.update(...).

            // Undo / Redo
            {toolbar_button(__scope, TablerIcon::ArrowBackUp, "Undo (Ctrl+Z)", cmd_click!("undo"))}
            {toolbar_button(__scope, TablerIcon::ArrowForwardUp, "Redo (Ctrl+Shift+Z)", cmd_click!("redo"))}

            {separator(__scope)}

            // Image insert (keeps the server-upload flow; inserts via handle.insert_image)
            {toolbar_button(__scope, TablerIcon::Photo, "Insert Image", {
                let h = handle.clone();
                let book_id = book_id.clone();
                move || {
                    insert_image_via_picker(h.clone(), &book_id, on_edit);
                }
            })}
        }
    }
}

/// Opens a file picker, uploads the selected image (producing a server URL), and
/// inserts it into `handle` via `insert_image`. `on_edit` fires after insertion so
/// the caller can schedule an autosave.
fn insert_image_via_picker(handle: EditorHandle, book_id: &str, on_edit: impl Fn() + 'static + Copy) {
    let Some(doc) = crate::platform::window().and_then(|w| w.document()) else { return; };
    let Ok(input) = doc.create_element("input") else { return; };
    let input: web_sys::HtmlInputElement = input.unchecked_into();
    input.set_type("file");
    input.set_accept("image/*");

    let book_id = book_id.to_string();
    let onchange = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
        let input: web_sys::HtmlInputElement = crate::platform::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("__pw_image_input"))
            .map(|e| e.unchecked_into())
            .unwrap();
        let Some(files) = input.files() else { return; };
        let Some(file) = files.get(0) else { return; };
        let bid = book_id.clone();
        let handle = handle.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match crate::api::upload_image(&bid, &file).await {
                Ok(resp) => {
                    handle.insert_image(&resp.url, "");
                    on_edit();
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Image upload failed: {}", e.message).into());
                }
            }
        });
        // Clean up
        input.remove();
    }) as Box<dyn FnMut(_)>);
    input.set_id("__pw_image_input");
    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    doc.body().unwrap().append_child(&input).ok();
    input.click();
}

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

/// Allowlist of element tag names (lowercase) permitted in rendered prose.
/// Anything not in this list is unwrapped/removed by `sanitize_html`.
const ALLOWED_TAGS: &[&str] = &[
    "p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote",
    "hr", "br", "strong", "b", "em", "i", "code", "s", "del", "u", "mark",
    "ins", "sub", "sup", "a", "img", "span", "div",
];

/// Sanitize untrusted HTML produced from user prose before it is injected via
/// `set_inner_html`. `markdown_to_html` only converts a handful of inline markers
/// and copies the rest of the prose verbatim, so a literal `<img onerror=...>` or
/// `<script>` in the source would otherwise become live DOM (self-XSS).
///
/// Strategy: parse the HTML into a DETACHED element (never connected to the live
/// document, so no scripts run / no resources load), walk the resulting DOM tree,
/// drop disallowed elements (unwrapping their text children so prose survives),
/// and strip dangerous attributes (`on*` handlers, `javascript:` URLs). The
/// cleaned element's innerHTML is returned.
pub fn sanitize_html(raw: &str) -> String {
    let doc = match crate::platform::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return String::new(),
    };
    let container = match doc.create_element("div") {
        Ok(el) => el,
        Err(_) => return String::new(),
    };
    // Setting innerHTML on a detached element parses but does NOT execute scripts
    // or fetch resources.
    container.set_inner_html(raw);

    sanitize_node(&container);

    container.inner_html()
}

/// Recursively sanitize the element children of `el` in place.
fn sanitize_node(el: &web_sys::Element) {
    use wasm_bindgen::JsCast;

    // Collect element children first; we mutate the tree as we go. Use child_nodes()
    // (NodeList) + an ELEMENT_NODE filter to avoid needing the HtmlCollection
    // web-sys feature. Non-element nodes (text, comments) are left untouched.
    let child_nodes = el.child_nodes();
    let mut nodes: Vec<web_sys::Element> = Vec::new();
    for i in 0..child_nodes.length() {
        if let Some(node) = child_nodes.item(i) {
            if node.node_type() == web_sys::Node::ELEMENT_NODE {
                if let Ok(child_el) = node.dyn_into::<web_sys::Element>() {
                    nodes.push(child_el);
                }
            }
        }
    }

    for child in nodes {
        let tag = child.tag_name().to_lowercase();
        if !ALLOWED_TAGS.contains(&tag.as_str()) {
            // Disallowed element (script, iframe, svg, object, embed, style, ...).
            // Remove it entirely. We intentionally do NOT preserve children of
            // dangerous containers (e.g. <script> text) to avoid leaking payloads.
            child.remove();
            continue;
        }

        // Strip dangerous attributes on the allowed element.
        let attrs = child.get_attribute_names();
        let mut to_remove: Vec<String> = Vec::new();
        for i in 0..attrs.length() {
            if let Some(name) = attrs.get(i).as_string() {
                let lower = name.to_lowercase();
                if lower.starts_with("on") {
                    to_remove.push(name);
                    continue;
                }
                if lower == "href" || lower == "src" || lower == "xlink:href" {
                    let val = child.get_attribute(&name).unwrap_or_default();
                    let trimmed = val.trim_start().to_lowercase();
                    if trimmed.starts_with("javascript:")
                        || trimmed.starts_with("data:text/html")
                        || trimmed.starts_with("vbscript:")
                    {
                        to_remove.push(name);
                    }
                }
            }
        }
        for name in to_remove {
            child.remove_attribute(&name).ok();
        }

        // Recurse into the (now attribute-cleaned) allowed element.
        sanitize_node(&child);
    }
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

