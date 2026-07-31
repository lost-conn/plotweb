use std::rc::Rc;

use rinch::prelude::*;
use rinch_core::Signal;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use crate::rinch_backend::EditorHandle;
use rinch_editor_core::serialize::DocNode;

use wasm_bindgen::prelude::*;

// The one canonical legacy-Markdown → HTML converter now lives in `plotweb-common`
// so the migration audit (`plotweb-crdt`) converts legacy chapter bodies exactly
// the way the editor does here. Re-exported so existing `editor_utils::markdown_to_html`
// callers keep working.
pub use plotweb_common::markdown_to_html;

// ── Model-first editor (rinch-editor-view) content bridge ──────────────────
//
// Chapters/notes are stored as an **opaque String** the frontend owns. New saves
// are `DocNode` JSON (the editor's durable wire shape); legacy content is still
// chapter Markdown / note HTML. Loads are legacy-tolerant: try DocNode JSON first,
// fall back to the pre-8a HTML path. A later card bulk-migrates legacy content.

/// Detach any collaboration session before replacing this editor's document.
///
/// **Load-bearing.** An attached session records document changes into that
/// document's CRDT, and a load is a document change — `start_collaboration_guest`
/// relies on exactly this, loading the peer's document *before* attaching so the
/// load isn't recorded back. So loading chapter B into an editor still bound to
/// chapter A's session writes B's content into **A's** local document.
///
/// That was the chapter-crosstalk bug: each switch overwrote the previous chapter's
/// local doc with the next chapter's content, and since a local doc is adopted in
/// preference to the server copy when a chapter is reopened, chapters appeared
/// swapped or blank even though the server held the right text. Detaching is
/// synchronous and belongs *here*, at the single choke point every load goes
/// through, rather than at each call site where a future one could forget it.
/// `local_store` re-attaches after it has decided which document this editor holds.
fn detach_before_load(handle: &EditorHandle) {
    handle.stop_collaboration();
}

/// Load stored chapter content into `handle`. Tries `DocNode` JSON (new format),
/// falling back to the legacy Markdown → HTML path (`markdown_to_html`).
pub fn load_chapter_content(handle: &EditorHandle, content: &str) {
    detach_before_load(handle);
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
    detach_before_load(handle);
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

/// Allowlist of element tag names (lowercase) permitted in rendered prose.
/// Anything not in this list is unwrapped/removed by `sanitize_html`.
/// Web-only: the native `sanitize_html` passes through (no JS engine to guard).
#[cfg(target_arch = "wasm32")]
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
///
/// # Why this is web-only (see the native branch below)
///
/// The sanitizer exists to stop `<script>`, `on*=` handlers and `javascript:`
/// URLs — smuggled in via a legacy note's pasted HTML — from becoming live DOM.
/// Every one of those vectors needs a JavaScript engine to fire. The native
/// build has none: rinch-dom's `set_inner_html` runs `html_parser::parse_html_string`
/// and builds a *widget* tree, so an `on*` attribute is only ever stored as an
/// inert string in the node's attribute map (`dom_document_impl.rs::set_attribute`)
/// and nothing in the event system ever reads it — native handlers come solely
/// from Rust callbacks registered through `register_handler`. A `<script>` tag
/// is likewise just a node whose text is never executed (and which is laid out
/// `display: none`). So the XSS this guards against is strictly a *web* concern.
///
/// Therefore: on web, sanitize exactly as before; on native, pass through. The
/// alternative — the old behavior of returning `String::new()` off-wasm — rendered
/// the reader blank, which is strictly worse than rendering the author's own prose.
#[cfg(target_arch = "wasm32")]
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

/// Native pass-through: there is no JS engine to sanitize *against*.
///
/// rinch-dom parses this HTML into widgets; `on*` attributes are inert strings
/// and `<script>` text is never executed, so the script/handler/`javascript:`
/// vectors the web sanitizer strips cannot fire here. See the doc comment on the
/// wasm version above for the full reasoning.
///
/// Passing through also needs no DOM: the web implementation used a detached
/// `web_sys` div purely as an HTML *parser*, and `web_sys` is unavailable off-wasm.
#[cfg(not(target_arch = "wasm32"))]
pub fn sanitize_html(raw: &str) -> String {
    raw.to_string()
}

/// Recursively sanitize the element children of `el` in place.
#[cfg(target_arch = "wasm32")]
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


