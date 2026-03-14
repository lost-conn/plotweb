use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{
    BetaFeedback, BetaReaderLink, Book, Chapter, CreateBetaLinkRequest,
    CreateBetaReplyRequest, CreateChapterRequest, FontSettings,
    ReorderChaptersRequest, UpdateBetaLinkRequest, UpdateBookRequest, UpdateChapterRequest,
};

use crate::api;
use crate::fonts;
use crate::pages::editor_utils;
use crate::router;
use crate::store::{AppStore, Route};

use rinch_core::Signal;

/// What the main pane shows.
#[derive(Clone, PartialEq)]
enum BookPane {
    Chapters,
    Editor(String),
    Typography,
    BetaReaders,
}

/// CSS for the typography settings section.
const TYPOGRAPHY_CSS: &str = r#"
.typography-section {
    padding: 16px 0;
}

.font-selector-grid {
    display: grid;
    grid-template-columns: 120px 1fr;
    gap: 8px 16px;
    align-items: center;
}

.font-selector-label {
    font-size: 13px;
    color: var(--rinch-color-dimmed);
    text-align: right;
}

.font-picker {
    position: relative;
}

.font-picker input {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 10px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-surface);
    color: var(--rinch-color-text);
    font-size: 13px;
    outline: none;
    transition: border-color 0.15s;
}
.font-picker input:focus {
    border-color: var(--rinch-color-teal-6);
}
.font-picker input::placeholder {
    color: var(--rinch-color-dimmed);
    opacity: 0.7;
}

.font-picker .font-dropdown {
    display: none;
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    max-height: 240px;
    overflow-y: auto;
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-top: none;
    border-radius: 0 0 var(--rinch-radius-sm) var(--rinch-radius-sm);
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
}
.font-picker .font-dropdown.open {
    display: block;
}

.font-dropdown .font-option {
    padding: 6px 10px;
    font-size: 13px;
    cursor: pointer;
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: var(--rinch-color-text);
}
.font-dropdown .font-option:hover,
.font-dropdown .font-option.active {
    background: var(--rinch-color-teal-9);
}
.font-dropdown .font-option .font-category {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
    margin-left: 8px;
    flex-shrink: 0;
}
.font-dropdown .font-option-default {
    padding: 6px 10px;
    font-size: 13px;
    cursor: pointer;
    color: var(--rinch-color-dimmed);
    font-style: italic;
    border-bottom: 1px solid var(--rinch-color-border);
}
.font-dropdown .font-option-default:hover {
    background: var(--rinch-color-teal-9);
}
.font-dropdown .font-empty {
    padding: 12px 10px;
    font-size: 13px;
    color: var(--rinch-color-dimmed);
    text-align: center;
}
.font-dropdown .font-loading {
    padding: 12px 10px;
    font-size: 13px;
    color: var(--rinch-color-dimmed);
    text-align: center;
}

.pw-typo-select {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 10px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-surface);
    color: var(--rinch-color-text);
    font-size: 13px;
    outline: none;
    cursor: pointer;
    transition: border-color 0.15s;
}
.pw-typo-select:focus {
    border-color: var(--rinch-color-teal-6);
}

.font-preview-box {
    margin-top: 16px;
    padding: 20px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-body);
}

.font-preview-box h3 {
    font-size: 1.5em;
    margin: 0 0 8px 0;
    font-weight: 700;
}

.font-preview-box h4 {
    font-size: 1.15em;
    margin: 0 0 8px 0;
    font-weight: 600;
    color: var(--rinch-color-dimmed);
}

.font-preview-box .preview-body {
    margin: 0 0 8px 0;
    line-height: 1.7;
}

.font-preview-box blockquote {
    border-left: 3px solid var(--rinch-color-teal-8);
    padding-left: 12px;
    margin: 8px 0;
    color: var(--rinch-color-dimmed);
}

.font-preview-box code {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 0.9em;
}
"#;

/// CSS for the book workspace layout.
const BOOK_WORKSPACE_CSS: &str = r#"
.book-workspace {
    display: flex;
    height: 100vh;
}

.book-sidebar {
    width: 250px;
    min-width: 250px;
    padding: var(--rinch-spacing-md) var(--rinch-spacing-md) var(--rinch-spacing-sm);
    border-right: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
}

.book-sidebar-title {
    padding: 4px var(--rinch-spacing-xs);
    margin-bottom: var(--rinch-spacing-xs);
    font-weight: 700;
    font-size: 16px;
    color: var(--rinch-color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: 'Macondo Swash Caps', cursive;
}

.book-sidebar-nav {
    flex: 1;
    padding-top: var(--rinch-spacing-xs);
}

.sidebar-section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px var(--rinch-spacing-xs);
    margin-top: var(--rinch-spacing-sm);
    cursor: pointer;
    color: var(--rinch-color-dimmed);
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    transition: color 0.15s;
}

.sidebar-section-header:hover {
    color: var(--rinch-color-text);
}

.sidebar-section-header.active {
    color: var(--rinch-color-teal-4);
}

.sidebar-chapter-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 4px 0;
}

.sidebar-chapter-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px 6px 16px;
    border-radius: var(--rinch-radius-sm);
    cursor: pointer;
    font-size: 14px;
    color: var(--rinch-color-text);
    transition: background 0.12s ease;
}

.sidebar-chapter-item:hover {
    background: var(--rinch-color-border);
}

.sidebar-chapter-item.active {
    background: var(--rinch-color-teal-9);
    color: var(--rinch-color-teal-3);
}

.sidebar-chapter-actions {
    display: flex;
    gap: 1px;
    opacity: 0;
    transition: opacity 0.15s;
}

.sidebar-chapter-item:hover .sidebar-chapter-actions {
    opacity: 1;
}

.sidebar-chapter-name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.book-sidebar-footer {
    padding: var(--rinch-spacing-sm) var(--rinch-spacing-xs);
    margin-top: var(--rinch-spacing-sm);
    border-top: 1px solid var(--rinch-color-border);
    display: flex;
    flex-direction: column;
    gap: 8px;
}

.book-sidebar-footer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.book-main-pane {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--rinch-color-body);
}

.book-main-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 40px 48px;
}

/* ── Chapters pane ──────────────────────────────────── */

.chapters-pane {
    max-width: 720px;
    margin: 0 auto;
}

.chapters-pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--rinch-spacing-md);
}

.chapter-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
}

.chapter-item {
    cursor: pointer;
    transition: background 0.12s ease, border-color 0.12s ease;
    border: 1px solid transparent;
    background: var(--rinch-color-surface);
}

.chapter-item:hover {
    background: var(--rinch-color-border);
    border-color: var(--rinch-color-border);
}

.chapter-item-content {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.chapter-item-left {
    display: flex;
    align-items: center;
    gap: var(--rinch-spacing-sm);
    flex: 1;
    cursor: pointer;
    padding: 2px 0;
}

.chapter-item-actions {
    display: flex;
    gap: 2px;
    opacity: 0.4;
    transition: opacity 0.15s ease;
}

.chapter-item:hover .chapter-item-actions {
    opacity: 1;
}

/* ── Mobile hamburger bar ──────────────────────────── */

.mobile-topbar {
    display: none;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    border-bottom: 1px solid var(--rinch-color-border);
    background: var(--pw-color-deep);
    flex-shrink: 0;
}

.sidebar-backdrop {
    display: none;
}

.editor-feedback-backdrop {
    display: none;
}

@media (max-width: 768px) {
    .book-sidebar {
        display: none;
        position: fixed;
        top: 0;
        left: 0;
        bottom: 0;
        z-index: 200;
        width: 280px;
        min-width: 280px;
    }
    .book-sidebar.open {
        display: flex;
    }
    .sidebar-backdrop {
        display: none;
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        background: rgba(0, 0, 0, 0.5);
        z-index: 199;
    }
    .sidebar-backdrop.open {
        display: block;
    }
    .mobile-topbar {
        display: flex;
    }
    .book-main-scroll {
        padding: 24px 16px;
    }

    /* Editor feedback bottom sheet */
    .editor-feedback-sidebar {
        position: fixed; bottom: 0; left: 0; right: 0;
        height: 60vh; z-index: 200;
        width: 100% !important; min-width: 100% !important;
        border-radius: 12px 12px 0 0;
        border-top: 1px solid var(--rinch-color-border);
        border-left: none;
    }
    .editor-feedback-sidebar.hidden { display: none !important; }
    .editor-feedback-sidebar.visible { display: flex !important; }
    .editor-feedback-backdrop.open {
        display: block;
        position: fixed; top: 0; left: 0; right: 0; bottom: 0;
        background: rgba(0,0,0,0.3); z-index: 199;
    }
}

/* ── Beta Reader Cards ────────────────────────────── */

.beta-link-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
}

.beta-link-card {
    background: var(--rinch-color-surface);
    transition: opacity 0.2s;
}

.beta-link-card.inactive {
    opacity: 0.5;
}

.beta-link-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 4px;
}

.beta-link-meta {
    padding-top: 4px;
}

/* ── Editor Feedback Sidebar ──────────────────────── */

.editor-feedback-sidebar {
    width: 300px;
    min-width: 300px;
    background: var(--pw-color-deep);
    border-left: 1px solid var(--rinch-color-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

.editor-feedback-header {
    padding: 10px 14px;
    border-bottom: 1px solid var(--rinch-color-border);
    font-weight: 600;
    font-size: 13px;
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.editor-feedback-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
}

.feedback-card {
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-radius: 6px;
    padding: 10px 12px;
    margin-bottom: 8px;
    font-size: 13px;
}

.feedback-card.resolved {
    opacity: 0.4;
}

.feedback-reader-name {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--rinch-color-teal-4);
    margin-bottom: 4px;
}

.feedback-reader-name svg {
    width: 14px;
    height: 14px;
}

.feedback-quote {
    font-style: italic;
    color: var(--rinch-color-teal-4);
    font-size: 12px;
    padding: 4px 8px;
    border-left: 2px solid var(--rinch-color-teal-7);
    margin-bottom: 6px;
    word-break: break-word;
    cursor: pointer;
}

.feedback-quote:hover {
    background: var(--rinch-color-teal-9);
    border-radius: 3px;
}

@keyframes feedback-highlight-flash {
    0% { background-color: rgba(0, 128, 128, 0.4); }
    100% { background-color: transparent; }
}

.feedback-highlight {
    animation: feedback-highlight-flash 2s ease-out forwards;
    border-radius: 2px;
}

.feedback-comment {
    color: var(--rinch-color-text);
    margin-bottom: 4px;
    word-break: break-word;
}

.feedback-meta {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
}

.feedback-replies {
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--rinch-color-border);
}

.feedback-reply {
    padding: 2px 0;
    font-size: 12px;
}

.feedback-reply-author {
    font-weight: 600;
    color: var(--rinch-color-teal-4);
}

.feedback-reply-author.owner {
    color: var(--rinch-color-teal-3);
}

.feedback-actions {
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--rinch-color-border);
}

.feedback-reply-input {
    display: flex;
    gap: 4px;
}

.feedback-reply-input textarea {
    flex: 1;
    padding: 4px 8px;
    border: 1px solid var(--rinch-color-border);
    border-radius: 4px;
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    font-size: 12px;
    font-family: inherit;
    outline: none;
    resize: none;
    min-height: 28px;
    max-height: 120px;
    overflow-y: auto;
}

.feedback-reply-input textarea:focus {
    border-color: var(--rinch-color-teal-6);
}
"#;

/// Slot definitions for font pickers
const SLOTS: &[(&str, &str, &str, &str)] = &[
    ("h1", "Heading 1", "Macondo Swash Caps", ""),
    ("h2", "Heading 2", "Macondo Swash Caps", ""),
    ("h3", "Heading 3+", "Macondo Swash Caps", ""),
    ("body", "Body Text", "Playwrite DE Grund", ""),
    ("quote", "Blockquote", "inherit", ""),
    ("code", "Code", "monospace", "monospace"),
];

const SPACING_OPTIONS: &[(f64, &str)] = &[
    (0.0, "None"),
    (4.0, "Tight"),
    (8.0, "Normal"),
    (16.0, "Relaxed"),
    (24.0, "Loose"),
];

const INDENT_OPTIONS: &[(f64, &str)] = &[
    (0.0, "None"),
    (16.0, "Small"),
    (24.0, "Medium"),
    (32.0, "Large"),
    (48.0, "Extra"),
];

/// Build the HTML for the font picker grid.
fn build_font_grid_html(fs: &FontSettings) -> String {
    let mut html = String::new();
    for &(slot, label, default, _) in SLOTS {
        let current = match slot {
            "h1" => fs.h1.as_deref(),
            "h2" => fs.h2.as_deref(),
            "h3" => fs.h3.as_deref(),
            "body" => fs.body.as_deref(),
            "quote" => fs.quote.as_deref(),
            "code" => fs.code.as_deref(),
            _ => None,
        };
        let display_value = current.unwrap_or("");
        let placeholder = format!("Search fonts... (default: {})", default);
        html.push_str(&format!(
            "<span class='font-selector-label'>{label}</span>\
             <div class='font-picker' data-font-slot='{slot}'>\
               <input type='text' value='{value}' placeholder='{placeholder}' autocomplete='off' />\
               <div class='font-dropdown'></div>\
             </div>",
            label = label,
            slot = slot,
            value = display_value,
            placeholder = placeholder,
        ));
    }
    html
}

fn build_select_html(id: &str, options: &[(f64, &str)], current: f64) -> String {
    let mut html = format!("<select id='{id}' class='pw-typo-select'>");
    for &(val, label) in options {
        let selected = if (val - current).abs() < 0.01 { " selected" } else { "" };
        html.push_str(&format!("<option value='{val}'{selected}>{label}</option>"));
    }
    html.push_str("</select>");
    html
}

/// Build the HTML for the spacing settings grid.
fn build_spacing_grid_html(fs: &FontSettings) -> String {
    let mut html = String::new();

    html.push_str("<span class='font-selector-label'>Paragraph</span>");
    html.push_str(&build_select_html("pw-paragraph-spacing", SPACING_OPTIONS, fs.paragraph_spacing.unwrap_or(8.0)));

    html.push_str("<span class='font-selector-label'>Body Indent</span>");
    html.push_str(&build_select_html("pw-paragraph-indent", INDENT_OPTIONS, fs.paragraph_indent.unwrap_or(0.0)));

    html.push_str("<span class='font-selector-label'>Heading Indent</span>");
    html.push_str(&build_select_html("pw-heading-indent", INDENT_OPTIONS, fs.heading_indent.unwrap_or(0.0)));

    html
}

/// Update the font preview box styles.
fn update_preview(fs: &FontSettings) {
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };

    if let Ok(Some(el)) = document.query_selector("#font-preview") {
        let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
        let h2 = fs.h2.as_deref().unwrap_or("Macondo Swash Caps");
        let body = fs.body.as_deref().unwrap_or("Playwrite DE Grund");
        let quote = fs.quote.as_deref().unwrap_or("inherit");
        let code = fs.code.as_deref().unwrap_or("monospace");
        let p_spacing = fs.paragraph_spacing.unwrap_or(8.0);
        let p_indent = fs.paragraph_indent.unwrap_or(0.0);
        let h_indent = fs.heading_indent.unwrap_or(0.0);

        el.set_inner_html(&format!(
            "<h3 style=\"font-family: '{}', cursive; text-indent: {}px\">Chapter Title</h3>\
             <h4 style=\"font-family: '{}', cursive; text-indent: {}px\">Scene Heading</h4>\
             <p class='preview-body' style=\"font-family: '{}', serif; margin-bottom: {}px; text-indent: {}px\">The quick brown fox jumps over the lazy dog. She stared out the window, watching the rain trace paths down the glass.</p>\
             <p class='preview-body' style=\"font-family: '{}', serif; margin-bottom: {}px; text-indent: {}px\">The next morning, she found the letter on the doorstep, its edges curled and damp from the night air.</p>\
             <blockquote style=\"font-family: '{}', serif\">\u{201c}All that glitters is not gold.\u{201d}</blockquote>\
             <p><code style=\"font-family: '{}', monospace\">const story = new Adventure();</code></p>",
            h1, h_indent, h2, h_indent, body, p_spacing, p_indent, body, p_spacing, p_indent, quote, code
        ));
    }
}

/// Render a single sidebar chapter item. Extracted to avoid rsx for-loop move issues.
fn sidebar_chapter_item<O, M, FO, FM>(
    __scope: &mut RenderScope,
    id: String,
    title: String,
    active_pane: Signal<BookPane>,
    open_chapter: O,
    move_chapter: M,
) -> NodeHandle
where
    O: Fn(String) -> FO + 'static + Copy,
    M: Fn(String, i32) -> FM + 'static + Copy,
    FO: Fn() + 'static,
    FM: Fn() + 'static,
{
    let cid = id.clone();
    let cid2 = id.clone();
    let cid3 = id.clone();
    rsx! {
        div {
            key: id.clone(),
            class: {
                let cid = id.clone();
                move || {
                    if active_pane.get() == BookPane::Editor(cid.clone()) {
                        "sidebar-chapter-item active"
                    } else {
                        "sidebar-chapter-item"
                    }
                }
            },

            div {
                class: "sidebar-chapter-name",
                onclick: open_chapter(cid),
                {title}
            }
            div { class: "sidebar-chapter-actions",
                ActionIcon {
                    variant: "subtle",
                    size: "xs",
                    onclick: move_chapter(cid2, -1),
                    {render_tabler_icon(__scope, TablerIcon::ChevronUp, TablerIconStyle::Outline)}
                }
                ActionIcon {
                    variant: "subtle",
                    size: "xs",
                    onclick: move_chapter(cid3, 1),
                    {render_tabler_icon(__scope, TablerIcon::ChevronDown, TablerIconStyle::Outline)}
                }
            }
        }
    }
}

/// Scroll to and highlight a piece of text within the editor.
/// Walks all text nodes in `#editor-main`, finds a match for `selected_text`
/// (disambiguating with `context_block` if there are multiple matches),
/// creates a Selection over it, scrolls it into view, and applies a flash highlight.
fn scroll_to_text_in_editor(selected_text: &str, context_block: &str) {
    log::info!("scroll_to_text_in_editor called, text='{}'", &selected_text[..selected_text.len().min(40)]);
    if selected_text.is_empty() {
        return;
    }

    let doc = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => { log::warn!("scroll_to_text: no document"); return; }
    };
    let editor = match doc.query_selector("#editor-main").ok().flatten() {
        Some(el) => el,
        None => { log::warn!("scroll_to_text: no #editor-main"); return; }
    };

    // Collect all text content with node boundaries
    let mut text_nodes: Vec<web_sys::Node> = Vec::new();
    let mut full_text = String::new();
    let mut node_offsets: Vec<(usize, usize)> = Vec::new(); // (start_byte, end_byte) in full_text

    collect_text_nodes(&editor.into(), &mut text_nodes);

    for node in &text_nodes {
        let start = full_text.len();
        let content = node.text_content().unwrap_or_default();
        full_text.push_str(&content);
        node_offsets.push((start, full_text.len()));
    }

    // Find all matches of selected_text in the full text
    let needle = selected_text;
    let mut matches: Vec<usize> = Vec::new();
    let mut search_start = 0;
    while let Some(pos) = full_text[search_start..].find(needle) {
        matches.push(search_start + pos);
        search_start += pos + 1;
    }

    if matches.is_empty() {
        log::warn!("scroll_to_text: '{}' not found in editor content ({} chars)", &selected_text[..selected_text.len().min(40)], full_text.len());
        return;
    }
    log::info!("scroll_to_text: found {} match(es)", matches.len());

    // Pick the best match using context_block for disambiguation
    let match_start = if matches.len() == 1 || context_block.is_empty() {
        matches[0]
    } else {
        // Find the match whose surrounding text best matches context_block
        *matches.iter().min_by_key(|&&pos| {
            // Extract surrounding text
            let ctx_start = pos.saturating_sub(context_block.len());
            let ctx_end = (pos + needle.len() + context_block.len()).min(full_text.len());
            let surrounding = &full_text[ctx_start..ctx_end];
            // Simple: count how many chars of context_block appear in the surrounding text
            let overlap = context_block.chars()
                .filter(|c| surrounding.contains(*c))
                .count();
            context_block.len().saturating_sub(overlap)
        }).unwrap()
    };

    let match_end = match_start + needle.len();

    // Map byte offsets back to text node + byte offset within that node,
    // then convert to UTF-16 code unit offsets for the DOM Range/Text APIs.
    let (start_node_idx, start_byte_off) = byte_offset_to_node(&node_offsets, match_start);
    let (end_node_idx, end_byte_off) = byte_offset_to_node(&node_offsets, match_end);

    // Convert UTF-8 byte offsets to UTF-16 code unit offsets
    let start_text = text_nodes[start_node_idx].text_content().unwrap_or_default();
    let end_text = text_nodes[end_node_idx].text_content().unwrap_or_default();
    let start_u16 = utf8_byte_to_utf16(&start_text, start_byte_off);
    let end_u16 = utf8_byte_to_utf16(&end_text, end_byte_off);

    log::info!("scroll_to_text: node {}[{}] -> node {}[{}] (u16: {} -> {})",
        start_node_idx, start_byte_off, end_node_idx, end_byte_off, start_u16, end_u16);

    // Create a Range over the matched text
    let range = match doc.create_range() {
        Ok(r) => r,
        Err(e) => { log::warn!("scroll_to_text: create_range failed: {:?}", e); return; }
    };
    if let Err(e) = range.set_start(&text_nodes[start_node_idx], start_u16) {
        log::warn!("scroll_to_text: set_start failed: {:?}", e);
        return;
    }
    if let Err(e) = range.set_end(&text_nodes[end_node_idx], end_u16) {
        log::warn!("scroll_to_text: set_end failed: {:?}", e);
        return;
    }

    // Scroll the start of the match into view within .editor-scroll
    let start_parent = match text_nodes[start_node_idx].parent_element() {
        Some(el) => el,
        None => return,
    };
    if let Ok(Some(scroll_container)) = doc.query_selector(".editor-scroll") {
        let el_rect = start_parent.get_bounding_client_rect();
        let container_rect = scroll_container.get_bounding_client_rect();
        let scroll_top = scroll_container.scroll_top() as f64;
        // Scroll so the target is roughly 1/3 from the top of the container
        let target = scroll_top + el_rect.top() - container_rect.top()
            - container_rect.height() / 3.0;
        scroll_container.set_scroll_top(target.max(0.0) as i32);
    }

    // Select the range so it's visually highlighted with the browser's native selection
    if let Some(selection) = doc.get_selection().ok().flatten() {
        selection.remove_all_ranges().ok();
        selection.add_range(&range).ok();
    }

    // Also apply a temporary flash highlight to each text node in the range.
    // We wrap individual text nodes (not the whole range) to avoid the
    // cross-element-boundary error that surround_contents throws.
    let mut marks: Vec<web_sys::Element> = Vec::new();
    for idx in start_node_idx..=end_node_idx {
        if idx >= text_nodes.len() { break; }
        let node = &text_nodes[idx];
        let text = node.text_content().unwrap_or_default();
        if text.is_empty() { continue; }

        // Figure out the slice of this text node that's part of the match (in UTF-16 units)
        let node_u16_len: u32 = text.encode_utf16().count() as u32;
        let slice_start_u16 = if idx == start_node_idx { start_u16 } else { 0 };
        let slice_end_u16 = if idx == end_node_idx { end_u16 } else { node_u16_len };

        if slice_start_u16 == 0 && slice_end_u16 == node_u16_len {
            // Whole text node is in the match — wrap it directly
            if let Ok(mark) = doc.create_element("mark") {
                mark.set_class_name("feedback-highlight");
                if let Some(parent) = node.parent_node() {
                    parent.insert_before(&mark, Some(node)).ok();
                    mark.append_child(node).ok();
                    marks.push(mark);
                }
            }
        } else {
            // Partial text node — split and wrap the matched portion
            if let Ok(text_node) = node.clone().dyn_into::<web_sys::Text>() {
                // Split at slice_end first (so offsets stay valid), then at slice_start
                let _after = text_node.split_text(slice_end_u16).ok();
                let matched = text_node.split_text(slice_start_u16).ok();
                if let Some(matched_node) = matched {
                    if let Ok(mark) = doc.create_element("mark") {
                        mark.set_class_name("feedback-highlight");
                        if let Some(parent) = matched_node.parent_node() {
                            parent.insert_before(&mark, Some(&matched_node)).ok();
                            mark.append_child(&matched_node).ok();
                            marks.push(mark);
                        }
                    }
                }
            }
        }
    }

    // Remove marks after animation (2s)
    if !marks.is_empty() {
        let closure = wasm_bindgen::closure::Closure::once(move || {
            for mark in &marks {
                if let Some(parent) = mark.parent_node() {
                    while let Some(child) = mark.first_child() {
                        parent.insert_before(&child, Some(mark)).ok();
                    }
                    parent.remove_child(mark).ok();
                }
            }
            // Normalize to merge split text nodes back together
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                if let Ok(Some(editor)) = doc.query_selector("#editor-main") {
                    editor.normalize();
                }
            }
        });
        if let Some(window) = web_sys::window() {
            window.set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                2000,
            ).ok();
        }
        closure.forget();
    }
}

/// Convert a UTF-8 byte offset within a string to a UTF-16 code unit offset.
/// DOM APIs (Range.setStart, Text.splitText) use UTF-16 offsets,
/// but Rust's str::find returns UTF-8 byte positions.
fn utf8_byte_to_utf16(text: &str, byte_offset: usize) -> u32 {
    let clamped = byte_offset.min(text.len());
    text[..clamped].encode_utf16().count() as u32
}

/// Recursively collect all text nodes under `node`.
fn collect_text_nodes(node: &web_sys::Node, out: &mut Vec<web_sys::Node>) {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        out.push(node.clone());
        return;
    }
    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.item(i) {
            collect_text_nodes(&child, out);
        }
    }
}

/// Map a byte offset in the concatenated full text back to a (node_index, offset_within_node).
fn byte_offset_to_node(node_offsets: &[(usize, usize)], byte_offset: usize) -> (usize, usize) {
    for (i, &(start, end)) in node_offsets.iter().enumerate() {
        if byte_offset >= start && byte_offset <= end {
            return (i, byte_offset - start);
        }
    }
    // Fallback: last node, end
    let last = node_offsets.len().saturating_sub(1);
    (last, node_offsets.get(last).map(|o| o.1 - o.0).unwrap_or(0))
}

/// Render a feedback card in the editor sidebar.
fn editor_feedback_item<AR, RF, DF, ARO, RFO, DFO>(
    __scope: &mut RenderScope,
    fb: BetaFeedback,
    scroll_target: Signal<Option<(String, String)>>,
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
) -> NodeHandle
where
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
{
    let _fb_id = fb.id.clone();
    let fb_id3 = fb.id.clone();
    let fb_id4 = fb.id.clone();
    let fb_id5 = fb.id.clone();
    let class = if fb.resolved { "feedback-card resolved" } else { "feedback-card" };
    let reply_input_id = format!("author-reply-{}", fb.id);

    let reader_line = fb.reader_name.clone();
    let quote_style = if fb.selected_text.is_empty() { "display:none" } else { "" };
    // Store scroll data in a signal so the onclick closure only captures Copy types (signals)
    let scroll_data: Signal<(String, String)> = Signal::new((fb.selected_text.clone(), fb.context_block.clone()));
    let quote_line = if !fb.selected_text.is_empty() {
        let t = if fb.selected_text.len() > 80 { format!("{}...", &fb.selected_text[..80]) } else { fb.selected_text.clone() };
        format!("\u{201c}{}\u{201d}", t)
    } else { String::new() };
    let comment_line = fb.comment.clone();
    let reply_nodes: Vec<NodeHandle> = fb.replies.iter().map(|r| {
        book_reply_item(__scope, r.author_type.clone(), r.author_name.clone(), r.content.clone())
    }).collect();

    rsx! {
        div {
            class: class,
            key: _fb_id,
            div { class: "feedback-reader-name", {reader_line} }
            div { class: "feedback-quote", style: quote_style,
                onclick: move || {
                    let data = scroll_data.get();
                    // Set the signal (for cross-chapter navigation)
                    scroll_target.set(Some(data.clone()));
                    // Also immediately scroll (for same-chapter clicks)
                    let closure = wasm_bindgen::closure::Closure::once(move || {
                        scroll_to_text_in_editor(&data.0, &data.1);
                    });
                    if let Some(w) = web_sys::window() {
                        w.set_timeout_with_callback_and_timeout_and_arguments_0(
                            closure.as_ref().unchecked_ref(), 50,
                        ).ok();
                    }
                    closure.forget();
                },
                {quote_line}
            }
            div { class: "feedback-comment", {comment_line} }
            div { class: "feedback-replies",
                {reply_nodes}
            }
            div { class: "feedback-actions",
                div { class: "feedback-reply-input",
                    onsubmit: author_reply(fb_id3.clone()),
                    textarea {
                        id: reply_input_id,
                        placeholder: "Reply...",
                        rows: "1",
                    }
                    ActionIcon { variant: "subtle", size: "xs", onclick: author_reply(fb_id3),
                        {render_tabler_icon(__scope, TablerIcon::Send, TablerIconStyle::Outline)}
                    }
                }
                div { style: "display: flex; gap: 4px; margin-top: 4px;",
                    ActionIcon { variant: "subtle", size: "xs", onclick: resolve_feedback(fb_id4),
                        {render_tabler_icon(__scope, TablerIcon::Check, TablerIconStyle::Outline)}
                    }
                    ActionIcon { variant: "subtle", size: "xs", color: "red", onclick: delete_feedback(fb_id5),
                        {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                    }
                }
            }
        }
    }
}

/// Render a feedback card in the beta readers overview pane.
fn overview_feedback_item<AR, RF, DF, NF, ARO, RFO, DFO, NFO>(
    __scope: &mut RenderScope,
    fb: BetaFeedback,
    ch_title: String,
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
    navigate_to_feedback: NF,
) -> NodeHandle
where
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    NF: Fn(String, String, String) -> NFO + 'static + Copy,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
    NFO: Fn() + 'static,
{
    let _fb_id = fb.id.clone();
    let fb_id2 = fb.id.clone();
    let fb_id3 = fb.id.clone();
    let fb_id4 = fb.id.clone();
    let fb_id5 = fb.id.clone();
    let class = if fb.resolved { "feedback-card resolved" } else { "feedback-card" };
    let fb_reader = fb.reader_name.clone();
    let fb_comment = fb.comment.clone();
    let quote_style = if fb.selected_text.is_empty() { "display:none" } else { "" };
    let nav_chapter_id = fb.chapter_id.clone();
    let nav_selected_text = fb.selected_text.clone();
    let nav_context_block = fb.context_block.clone();
    let quote_text = if !fb.selected_text.is_empty() {
        let t = if fb.selected_text.len() > 100 { format!("{}...", &fb.selected_text[..100]) } else { fb.selected_text.clone() };
        format!("\u{201c}{}\u{201d}", t)
    } else { String::new() };
    let reply_nodes: Vec<NodeHandle> = fb.replies.iter().map(|r| {
        book_reply_item(__scope, r.author_type.clone(), r.author_name.clone(), r.content.clone())
    }).collect();

    rsx! {
        Paper {
            key: _fb_id,
            shadow: "xs",
            p: "sm",
            radius: "sm",
            class: class,

            div {
                style: "display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px;",
                div {
                    style: "display: flex; align-items: center; gap: 6px;",
                    Badge { variant: "light", size: "xs", {fb_reader} }
                    Text { size: "xs", color: "dimmed", {ch_title} }
                }
                div {
                    style: "display: flex; gap: 4px;",
                    ActionIcon {
                        variant: "subtle",
                        size: "xs",
                        onclick: resolve_feedback(fb_id2),
                        {render_tabler_icon(__scope, TablerIcon::Check, TablerIconStyle::Outline)}
                    }
                    ActionIcon {
                        variant: "subtle",
                        size: "xs",
                        color: "red",
                        onclick: delete_feedback(fb_id3),
                        {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                    }
                }
            }

            div { class: "feedback-quote", style: quote_style,
                onclick: navigate_to_feedback(nav_chapter_id, nav_selected_text, nav_context_block),
                {quote_text}
            }
            div { class: "feedback-comment", {fb_comment} }

            div { class: "feedback-replies",
                {reply_nodes}
            }

            div { class: "feedback-reply-input",
                onsubmit: author_reply(fb_id5.clone()),
                textarea {
                    id: {format!("author-reply-{}", fb_id4)},
                    placeholder: "Reply...",
                    rows: "1",
                }
                ActionIcon {
                    variant: "subtle",
                    size: "xs",
                    onclick: author_reply(fb_id5),
                    {render_tabler_icon(__scope, TablerIcon::Send, TablerIconStyle::Outline)}
                }
            }
        }
    }
}

fn book_reply_item(
    __scope: &mut RenderScope,
    author_type: String,
    author_name: String,
    content: String,
) -> NodeHandle {
    let class_str = if author_type == "owner" { "feedback-reply-author owner" } else { "feedback-reply-author" };
    rsx! {
        div { class: "feedback-reply",
            span { class: class_str, {format!("{}: ", author_name)} }
            {content}
        }
    }
}

fn render_feedback_overview<AR, RF, DF, NF, ARO, RFO, DFO, NFO>(
    __scope: &mut RenderScope,
    feedback: &[BetaFeedback],
    chapters: &[Chapter],
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
    navigate_to_feedback: NF,
) -> NodeHandle
where
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    NF: Fn(String, String, String) -> NFO + 'static + Copy,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
    NFO: Fn() + 'static,
{
    let nodes: Vec<NodeHandle> = feedback.iter().map(|fb| {
        let ch_title = chapters.iter()
            .find(|c| c.id == fb.chapter_id)
            .map(|c| c.title.clone())
            .unwrap_or_else(|| String::from("Unknown"));
        overview_feedback_item(__scope, fb.clone(), ch_title, author_reply, resolve_feedback, delete_feedback, navigate_to_feedback)
    }).collect();
    rsx! { {nodes} }
}

/// Switch to editing a chapter. Saves current chapter if editing, clears timers, loads new chapter.
/// All parameters are Copy or Clone so this can be called from rsx! closures without ownership issues.
fn do_switch_chapter(
    active_pane: Signal<BookPane>,
    auto_save_timer_id: Signal<i32>,
    save_status: Signal<&'static str>,
    editor_loaded: Signal<bool>,
    chapter_title: Signal<String>,
    store: AppStore,
    bid: &str,
    new_chapter_id: &str,
) {
    do_switch_chapter_inner(active_pane, auto_save_timer_id, save_status, editor_loaded, chapter_title, store, bid, new_chapter_id, None);
}

fn do_switch_chapter_inner(
    active_pane: Signal<BookPane>,
    auto_save_timer_id: Signal<i32>,
    save_status: Signal<&'static str>,
    editor_loaded: Signal<bool>,
    chapter_title: Signal<String>,
    store: AppStore,
    bid: &str,
    new_chapter_id: &str,
    pending_scroll: Option<Signal<Option<(String, String)>>>,
) {
    // Clear any pending auto-save timer
    let prev = auto_save_timer_id.get();
    if prev != 0 {
        if let Some(w) = web_sys::window() {
            w.clear_timeout_with_handle(prev);
        }
        auto_save_timer_id.set(0);
    }

    // Save current chapter if we're editing
    if let BookPane::Editor(ref current_id) = active_pane.get() {
        let current_id = current_id.clone();
        let bid = bid.to_string();
        save_status.set("saving");
        wasm_bindgen_futures::spawn_local(async move {
            let content = match web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector("#editor-main").ok().flatten())
            {
                Some(el) => el.inner_html(),
                None => {
                    save_status.set("saved");
                    return;
                }
            };
            let markdown = editor_utils::html_to_markdown(&content);
            let req = UpdateChapterRequest { title: None, content: Some(markdown) };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", bid, current_id),
                &req,
            ).await.is_ok() {
                save_status.set("saved");
            }
        });
    }

    // Switch pane
    let new_cid = new_chapter_id.to_string();
    active_pane.set(BookPane::Editor(new_cid.clone()));
    save_status.set("saved");
    store.sidebar_open.set(false);

    // Load new chapter
    editor_loaded.set(false);
    let bid = bid.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(chapter) = api::get::<Chapter>(
            &format!("/api/books/{}/chapters/{}", bid, new_cid),
        ).await {
            chapter_title.set(chapter.title.clone());
            editor_loaded.set(true);

            let content = chapter.content.clone();
            let window = web_sys::window().unwrap();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                    if let Ok(Some(el)) = doc.query_selector("#editor-main") {
                        if content.is_empty() {
                            el.set_inner_html("<p></p>");
                        } else {
                            el.set_inner_html(&editor_utils::markdown_to_html(&content));
                        }
                    }
                }
                // Execute pending feedback scroll if any
                if let Some(scroll_signal) = pending_scroll {
                    if let Some((selected_text, context_block)) = scroll_signal.get() {
                        scroll_signal.set(None);
                        // Delay slightly to let the DOM settle after innerHTML
                        let closure2 = wasm_bindgen::closure::Closure::once(move || {
                            scroll_to_text_in_editor(&selected_text, &context_block);
                        });
                        if let Some(w) = web_sys::window() {
                            w.set_timeout_with_callback_and_timeout_and_arguments_0(
                                closure2.as_ref().unchecked_ref(),
                                50,
                            ).ok();
                        }
                        closure2.forget();
                    }
                }
            });
            window.set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                100,
            ).ok();
            closure.forget();
        }
    });
}

fn render_beta_link_card<CB, TG, DL, CBO, TGO, DLO>(
    __scope: &mut RenderScope,
    link: BetaReaderLink,
    edit_beta_reader_name: Signal<String>,
    edit_beta_max_chapter: Signal<Option<i64>>,
    edit_beta_pinned: Signal<bool>,
    editing_beta_link: Signal<Option<BetaReaderLink>>,
    copy_beta_link: CB,
    toggle_beta_link_active: TG,
    delete_beta_link: DL,
) -> NodeHandle
where
    CB: Fn(String) -> CBO + 'static + Copy,
    TG: Fn(String, bool) -> TGO + 'static + Copy,
    DL: Fn(String) -> DLO + 'static + Copy,
    CBO: Fn() + 'static,
    TGO: Fn() + 'static,
    DLO: Fn() + 'static,
{
    let _link_key = link.id.clone();
    let card_class = if link.active { "beta-link-card" } else { "beta-link-card inactive" };
    let link_name = link.reader_name.clone();
    let link_active = link.active;
    let link_token = link.token.clone();
    let link_id_for_toggle = link.id.clone();
    let link_id_for_delete = link.id.clone();
    let meta_text = if let Some(max) = link.max_chapter_index {
        format!("Access: up to chapter {} | Created: {}", max + 1, link.created_at)
    } else {
        format!("Access: all chapters | Created: {}", link.created_at)
    };
    let pin_badge = if let Some(ref commit) = link.pinned_commit {
        format!("Pinned: {}", &commit[..7.min(commit.len())])
    } else {
        String::new()
    };
    let is_pinned = link.pinned_commit.is_some();
    let edit_link = link;

    rsx! {
        Paper {
            key: link_key,
            shadow: "xs",
            p: "md",
            radius: "sm",
            class: card_class,

            div { class: "beta-link-header",
                div {
                    style: "display: flex; align-items: center; gap: 8px;",
                    {render_tabler_icon(__scope, TablerIcon::User, TablerIconStyle::Outline)}
                    Text { weight: "600", {link_name} }
                    if !link_active {
                        Badge { variant: "light", size: "xs", color: "red", "Inactive" }
                    }
                }
                div {
                    style: "display: flex; align-items: center; gap: 4px;",
                    ActionIcon {
                        variant: "subtle",
                        size: "sm",
                        onclick: move || {
                            edit_beta_reader_name.set(edit_link.reader_name.clone());
                            edit_beta_max_chapter.set(edit_link.max_chapter_index);
                            edit_beta_pinned.set(edit_link.pinned_commit.is_some());
                            editing_beta_link.set(Some(edit_link.clone()));
                        },
                        {render_tabler_icon(__scope, TablerIcon::Pencil, TablerIconStyle::Outline)}
                    }
                    ActionIcon {
                        variant: "subtle",
                        size: "sm",
                        onclick: copy_beta_link(link_token),
                        {render_tabler_icon(__scope, TablerIcon::Copy, TablerIconStyle::Outline)}
                    }
                    ActionIcon {
                        variant: "subtle",
                        size: "sm",
                        onclick: toggle_beta_link_active(link_id_for_toggle, link_active),
                        {render_tabler_icon(
                            __scope,
                            if link_active { TablerIcon::PlayerPause } else { TablerIcon::PlayerPlay },
                            TablerIconStyle::Outline,
                        )}
                    }
                    ActionIcon {
                        variant: "subtle",
                        color: "red",
                        size: "sm",
                        onclick: delete_beta_link(link_id_for_delete),
                        {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                    }
                }
            }
            div { class: "beta-link-meta",
                Text { size: "xs", color: "dimmed", {meta_text} }
                if is_pinned {
                    Badge { variant: "light", size: "xs", {pin_badge.clone()} }
                } else {
                    Badge { variant: "outline", size: "xs", "Live" }
                }
            }
        }
    }
}

#[component]
pub fn book_page(book_id: String) -> NodeHandle {
    let store = use_store::<AppStore>();
    let show_chapter_modal = Signal::new(false);
    let new_chapter_title = Signal::new(String::new());

    let active_pane: Signal<BookPane> = Signal::new(BookPane::Chapters);
    let chapters_collapsed = Signal::new(false);
    let chapter_title = Signal::new(String::new());
    let save_status = Signal::new("saved");
    let editor_loaded = Signal::new(false);
    let auto_save_timer_id = Signal::new(0_i32);

    let bid_signal = Signal::new(book_id.clone());
    let font_settings = Signal::new(FontSettings::default());
    let font_save_timer_id = Signal::new(0_i32);
    let listeners_attached = Signal::new(false);

    // Beta reader signals
    let beta_links: Signal<Vec<BetaReaderLink>> = Signal::new(Vec::new());
    let beta_feedback: Signal<Vec<BetaFeedback>> = Signal::new(Vec::new());
    let show_beta_link_modal = Signal::new(false);
    let new_beta_reader_name = Signal::new(String::new());
    let new_beta_max_chapter: Signal<Option<i64>> = Signal::new(None);
    let show_feedback_sidebar = Signal::new(false);
    let _beta_reply_text: Signal<String> = Signal::new(String::new());

    // Pending feedback scroll target: (selected_text, context_block)
    let pending_feedback_scroll: Signal<Option<(String, String)>> = Signal::new(None);

    // Edit beta link signals
    let editing_beta_link: Signal<Option<BetaReaderLink>> = Signal::new(None);
    let edit_beta_reader_name: Signal<String> = Signal::new(String::new());
    let edit_beta_max_chapter: Signal<Option<i64>> = Signal::new(None);
    let new_beta_pin_version: Signal<bool> = Signal::new(false);
    let edit_beta_pinned: Signal<bool> = Signal::new(false);

    // Trigger font catalog fetch
    fonts::fetch_font_catalog();

    // Fetch book and chapters
    let bid = book_id.clone();
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(book) = api::get::<Book>(&format!("/api/books/{}", bid)).await {
            let fs = book.font_settings.clone().unwrap_or_default();
            fonts::load_book_fonts(&fs);
            font_settings.set(fs);
            store.current_book.set(Some(book));
        }
        if let Ok(chapters) = api::get::<Vec<Chapter>>(&format!("/api/books/{}/chapters", bid)).await {
            store.chapters.set(chapters);
        }
        if let Ok(links) = api::get::<Vec<BetaReaderLink>>(&format!("/api/books/{}/beta-links", bid)).await {
            beta_links.set(links);
        }
        if let Ok(fb) = api::get::<Vec<BetaFeedback>>(&format!("/api/books/{}/feedback", bid)).await {
            beta_feedback.set(fb);
        }
    });

    // Connect WebSocket for real-time feedback
    {
        let bid = book_id.clone();
        let ws_url = crate::ws::ws_url(&format!("/api/books/{}/feedback/ws", bid));
        crate::ws::connect_feedback_ws(&ws_url, move |msg| {
            match msg {
                crate::ws::WsMessage::NewFeedback(fb) => {
                    beta_feedback.update(|list| {
                        if !list.iter().any(|f| f.id == fb.id) {
                            list.insert(0, fb);
                        }
                    });
                }
                crate::ws::WsMessage::NewReply { feedback_id, reply } => {
                    beta_feedback.update(|list| {
                        if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                            if !fb.replies.iter().any(|r| r.id == reply.id) {
                                fb.replies.push(reply);
                            }
                        }
                    });
                }
                crate::ws::WsMessage::FeedbackResolved { feedback_id, resolved } => {
                    beta_feedback.update(|list| {
                        if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                            fb.resolved = resolved;
                        }
                    });
                }
                crate::ws::WsMessage::FeedbackDeleted { feedback_id } => {
                    beta_feedback.update(|list| list.retain(|f| f.id != feedback_id));
                }
            }
        });
    }

    // ── Save helper for the editor ──────────────────────────────
    let save_content = move |chapter_id_to_save: String| {
        let bid = bid_signal.get();
        save_status.set("saving");
        wasm_bindgen_futures::spawn_local(async move {
            let content = match web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector("#editor-main").ok().flatten())
            {
                Some(el) => el.inner_html(),
                None => {
                    save_status.set("saved");
                    return;
                }
            };

            let markdown = editor_utils::html_to_markdown(&content);
            let req = UpdateChapterRequest {
                title: None,
                content: Some(markdown),
            };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", bid, chapter_id_to_save),
                &req,
            ).await.is_ok() {
                save_status.set("saved");
            } else {
                save_status.set("unsaved");
            }
        });
    };

    // Chapter switching is handled by do_switch_chapter() free function

    // ── Set up Ctrl+S and auto-save (once, on mount) ────────────
    {
        let save = save_content.clone();
        let window = web_sys::window().unwrap();

        // Ctrl+S
        let save_for_key = save.clone();
        let keydown = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if (event.ctrl_key() || event.meta_key()) && event.key() == "s" {
                event.prevent_default();
                if let BookPane::Editor(ref cid) = active_pane.get() {
                    save_for_key(cid.clone());
                }
            }
        }) as Box<dyn FnMut(_)>);
        window.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref()).ok();
        keydown.forget();

        // Auto-save on input with 3s debounce
        let save_for_input = save;
        let input_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::Event| {
            let dominated_by_editor = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .and_then(|el| el.closest(".editor-content[contenteditable]").ok().flatten())
                .is_some();
            if !dominated_by_editor {
                return;
            }

            save_status.set("unsaved");
            let prev = auto_save_timer_id.get();
            if prev != 0 {
                web_sys::window().unwrap().clear_timeout_with_handle(prev);
            }
            let save_fn = save_for_input.clone();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                if let BookPane::Editor(ref cid) = active_pane.get() {
                    save_fn(cid.clone());
                }
            });
            let id = web_sys::window().unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    closure.as_ref().unchecked_ref(),
                    3000,
                ).unwrap_or(0);
            closure.forget();
            auto_save_timer_id.set(id);
        }) as Box<dyn FnMut(_)>);
        let doc = window.document().unwrap();
        doc.add_event_listener_with_callback("input", input_closure.as_ref().unchecked_ref()).ok();
        input_closure.forget();
    }

    // ── Chapter actions ─────────────────────────────────────────
    let add_chapter = move || {
        let title = new_chapter_title.get();
        if title.trim().is_empty() {
            return;
        }
        show_chapter_modal.set(false);
        let bid = bid_signal.get();
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateChapterRequest { title };
            if let Ok(chapter) = api::post::<_, Chapter>(
                &format!("/api/books/{}/chapters", bid),
                &req,
            ).await {
                store.chapters.update(|ch| ch.push(chapter));
            }
        });
    };

    let move_chapter = move |chapter_id: String, direction: i32| {
        move || {
            store.chapters.update(|chapters| {
                let pos = chapters.iter().position(|c| c.id == chapter_id);
                if let Some(idx) = pos {
                    let new_idx = idx as i32 + direction;
                    if new_idx >= 0 && (new_idx as usize) < chapters.len() {
                        chapters.swap(idx, new_idx as usize);
                    }
                }
            });
            let ids: Vec<String> = store.chapters.get().iter().map(|c| c.id.clone()).collect();
            let bid = bid_signal.get();
            wasm_bindgen_futures::spawn_local(async move {
                let req = ReorderChaptersRequest { chapter_ids: ids };
                api::put::<_, serde_json::Value>(&format!("/api/books/{}/chapters/reorder", bid), &req).await.ok();
            });
        }
    };

    let delete_chapter = move |chapter_id: String| {
        move || {
            let cid = chapter_id.clone();
            let bid = bid_signal.get();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_req::<serde_json::Value>(
                    &format!("/api/books/{}/chapters/{}", bid, cid)
                ).await.is_ok() {
                    if active_pane.get() == BookPane::Editor(cid.clone()) {
                        active_pane.set(BookPane::Chapters);
                    }
                    store.chapters.update(|ch| ch.retain(|c| c.id != cid));
                }
            });
        }
    };

    let go_dashboard = move || {
        router::navigate(Route::Dashboard);
    };

    let logout = move || {
        wasm_bindgen_futures::spawn_local(async move {
            api::post::<_, serde_json::Value>("/api/auth/logout", &serde_json::json!({})).await.ok();
            store.current_user.set(None);
            router::navigate(Route::Login);
        });
    };

    let toggle_dark = move || {
        store.dark_mode.update(|d| *d = !*d);
    };

    let toggle_sidebar = move || {
        store.sidebar_open.update(|o| *o = !*o);
    };

    // ── Typography font picker setup ────────────────────────────
    let setup_font_pickers = move || {
        let bid_for_save = bid_signal.get();
        let closure = wasm_bindgen::closure::Closure::once(move || {
            if listeners_attached.get() {
                return;
            }
            let document = web_sys::window().unwrap().document().unwrap();

            let fs = font_settings.get();
            if let Ok(Some(grid)) = document.query_selector("#font-selector-grid") {
                grid.set_inner_html(&build_font_grid_html(&fs));
            }
            if let Ok(Some(grid)) = document.query_selector("#spacing-selector-grid") {
                grid.set_inner_html(&build_spacing_grid_html(&fs));
            }

            update_preview(&fs);

            for &(slot_name, _, _, cat_filter) in SLOTS {
                let selector = format!(".font-picker[data-font-slot='{}']", slot_name);
                let picker_el = match document.query_selector(&selector) {
                    Ok(Some(el)) => el,
                    _ => continue,
                };
                let input_el: web_sys::HtmlInputElement = match picker_el.query_selector("input") {
                    Ok(Some(el)) => el.dyn_into().unwrap(),
                    _ => continue,
                };
                let dropdown_el = match picker_el.query_selector(".font-dropdown") {
                    Ok(Some(el)) => el,
                    _ => continue,
                };

                let slot = slot_name.to_string();
                let cat = cat_filter.to_string();
                let bid = bid_for_save.clone();
                let input_ref = input_el.clone();
                let dropdown_ref = dropdown_el.clone();

                let apply_font = {
                    let slot = slot.clone();
                    let bid = bid.clone();
                    let input_ref = input_ref.clone();
                    let dropdown_ref = dropdown_ref.clone();
                    move |family: String| {
                        let new_value = if family.is_empty() { None } else { Some(family.clone()) };
                        input_ref.set_value(&family);
                        dropdown_ref.class_list().remove_1("open").ok();

                        let mut fs = font_settings.get();
                        match slot.as_str() {
                            "h1" => fs.h1 = new_value,
                            "h2" => fs.h2 = new_value,
                            "h3" => fs.h3 = new_value,
                            "body" => fs.body = new_value,
                            "quote" => fs.quote = new_value,
                            "code" => fs.code = new_value,
                            _ => {}
                        }

                        fonts::load_book_fonts(&fs);
                        font_settings.set(fs.clone());
                        store.current_book.update(|book| {
                            if let Some(b) = book {
                                b.font_settings = Some(fs.clone());
                            }
                        });

                        let prev = font_save_timer_id.get();
                        if prev != 0 {
                            if let Some(w) = web_sys::window() {
                                w.clear_timeout_with_handle(prev);
                            }
                        }
                        let bid = bid.clone();
                        let save_closure = wasm_bindgen::closure::Closure::once(move || {
                            wasm_bindgen_futures::spawn_local(async move {
                                let req = UpdateBookRequest {
                                    title: None,
                                    description: None,
                                    font_settings: Some(fs),
                                };
                                api::put::<_, serde_json::Value>(
                                    &format!("/api/books/{}", bid),
                                    &req,
                                ).await.ok();
                            });
                        });
                        let id = web_sys::window()
                            .unwrap()
                            .set_timeout_with_callback_and_timeout_and_arguments_0(
                                save_closure.as_ref().unchecked_ref(),
                                500,
                            )
                            .unwrap_or(0);
                        save_closure.forget();
                        font_save_timer_id.set(id);
                        update_preview(&font_settings.get());
                    }
                };

                // Input handler
                let apply_for_input = apply_font.clone();
                let dropdown_for_input = dropdown_ref.clone();
                let cat_for_input = cat.clone();
                let input_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    let query = input_ref.value().to_lowercase();
                    let catalog = fonts::font_catalog().get();
                    let dropdown = &dropdown_for_input;

                    if catalog.is_empty() {
                        dropdown.set_inner_html("<div class='font-loading'>Loading fonts...</div>");
                        dropdown.class_list().add_1("open").ok();
                        return;
                    }

                    let filtered: Vec<&fonts::FontInfo> = catalog.iter()
                        .filter(|f| {
                            if !cat_for_input.is_empty() && f.category != cat_for_input {
                                return false;
                            }
                            if query.is_empty() { return true; }
                            f.family.to_lowercase().contains(&query)
                        })
                        .take(50)
                        .collect();

                    let mut html = String::from(
                        "<div class='font-option-default' data-font-value=''>Reset to default</div>"
                    );
                    if filtered.is_empty() {
                        html.push_str("<div class='font-empty'>No fonts found</div>");
                    } else {
                        for f in &filtered {
                            html.push_str(&format!(
                                "<div class='font-option' data-font-value='{family}'>\
                                   <span>{family}</span>\
                                   <span class='font-category'>{cat}</span>\
                                 </div>",
                                family = f.family,
                                cat = f.category,
                            ));
                        }
                    }
                    dropdown.set_inner_html(&html);
                    dropdown.class_list().add_1("open").ok();

                    let options = dropdown.query_selector_all("[data-font-value]").unwrap();
                    for i in 0..options.length() {
                        if let Some(opt) = options.item(i) {
                            let el: web_sys::Element = opt.dyn_into().unwrap();
                            let value = el.get_attribute("data-font-value").unwrap_or_default();
                            let apply = apply_for_input.clone();
                            let click_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |e: web_sys::Event| {
                                e.prevent_default();
                                e.stop_propagation();
                                let v = value.clone();
                                if !v.is_empty() { fonts::load_single_font(&v); }
                                apply(v);
                            }) as Box<dyn FnMut(_)>);
                            el.add_event_listener_with_callback("mousedown", click_handler.as_ref().unchecked_ref()).ok();
                            click_handler.forget();
                        }
                    }
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("input", input_handler.as_ref().unchecked_ref()).ok();
                input_handler.forget();

                // Focus handler
                let input_for_focus = input_el.clone();
                let dropdown_for_focus = dropdown_ref.clone();
                let cat_for_focus = cat.clone();
                let apply_for_focus = apply_font.clone();
                let focus_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    let query = input_for_focus.value().to_lowercase();
                    let catalog = fonts::font_catalog().get();
                    let dropdown = &dropdown_for_focus;

                    if catalog.is_empty() {
                        dropdown.set_inner_html("<div class='font-loading'>Loading fonts...</div>");
                        dropdown.class_list().add_1("open").ok();
                        return;
                    }

                    let filtered: Vec<&fonts::FontInfo> = catalog.iter()
                        .filter(|f| {
                            if !cat_for_focus.is_empty() && f.category != cat_for_focus {
                                return false;
                            }
                            if query.is_empty() { return true; }
                            f.family.to_lowercase().contains(&query)
                        })
                        .take(50)
                        .collect();

                    let mut html = String::from(
                        "<div class='font-option-default' data-font-value=''>Reset to default</div>"
                    );
                    if filtered.is_empty() {
                        html.push_str("<div class='font-empty'>No fonts found</div>");
                    } else {
                        for f in &filtered {
                            html.push_str(&format!(
                                "<div class='font-option' data-font-value='{family}'>\
                                   <span>{family}</span>\
                                   <span class='font-category'>{cat}</span>\
                                 </div>",
                                family = f.family,
                                cat = f.category,
                            ));
                        }
                    }
                    dropdown.set_inner_html(&html);
                    dropdown.class_list().add_1("open").ok();

                    let options = dropdown.query_selector_all("[data-font-value]").unwrap();
                    for i in 0..options.length() {
                        if let Some(opt) = options.item(i) {
                            let el: web_sys::Element = opt.dyn_into().unwrap();
                            let value = el.get_attribute("data-font-value").unwrap_or_default();
                            let apply = apply_for_focus.clone();
                            let click_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |e: web_sys::Event| {
                                e.prevent_default();
                                e.stop_propagation();
                                let v = value.clone();
                                if !v.is_empty() { fonts::load_single_font(&v); }
                                apply(v);
                            }) as Box<dyn FnMut(_)>);
                            el.add_event_listener_with_callback("mousedown", click_handler.as_ref().unchecked_ref()).ok();
                            click_handler.forget();
                        }
                    }
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("focus", focus_handler.as_ref().unchecked_ref()).ok();
                focus_handler.forget();

                // Blur handler
                let dropdown_for_blur = dropdown_ref.clone();
                let blur_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    dropdown_for_blur.class_list().remove_1("open").ok();
                }) as Box<dyn FnMut(_)>);
                input_el.add_event_listener_with_callback("blur", blur_handler.as_ref().unchecked_ref()).ok();
                blur_handler.forget();
            }

            // ── Paragraph spacing/indent select handlers ──
            let save_typography = {
                let bid = bid_for_save.clone();
                move |fs: FontSettings| {
                    fonts::load_book_fonts(&fs);
                    font_settings.set(fs.clone());
                    store.current_book.update(|book| {
                        if let Some(b) = book {
                            b.font_settings = Some(fs.clone());
                        }
                    });
                    let prev = font_save_timer_id.get();
                    if prev != 0 {
                        if let Some(w) = web_sys::window() {
                            w.clear_timeout_with_handle(prev);
                        }
                    }
                    let bid = bid.clone();
                    let save_closure = wasm_bindgen::closure::Closure::once(move || {
                        wasm_bindgen_futures::spawn_local(async move {
                            let req = UpdateBookRequest {
                                title: None,
                                description: None,
                                font_settings: Some(fs),
                            };
                            api::put::<_, serde_json::Value>(
                                &format!("/api/books/{}", bid),
                                &req,
                            ).await.ok();
                        });
                    });
                    let id = web_sys::window()
                        .unwrap()
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            save_closure.as_ref().unchecked_ref(),
                            500,
                        )
                        .unwrap_or(0);
                    save_closure.forget();
                    font_save_timer_id.set(id);
                    update_preview(&font_settings.get());
                }
            };

            if let Ok(Some(el)) = document.query_selector("#pw-paragraph-spacing") {
                let save = save_typography.clone();
                let doc = document.clone();
                let handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                    let el: web_sys::HtmlSelectElement = doc.query_selector("#pw-paragraph-spacing")
                        .ok().flatten().unwrap().dyn_into().unwrap();
                    let val: f64 = el.value().parse().unwrap_or(8.0);
                    let mut fs = font_settings.get();
                    fs.paragraph_spacing = if (val - 8.0).abs() < 0.01 { None } else { Some(val) };
                    save(fs);
                }) as Box<dyn FnMut(_)>);
                el.add_event_listener_with_callback("change", handler.as_ref().unchecked_ref()).ok();
                handler.forget();
            }

            if let Ok(Some(el)) = document.query_selector("#pw-paragraph-indent") {
                let save = save_typography.clone();
                let doc = document.clone();
                let handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                    let el: web_sys::HtmlSelectElement = doc.query_selector("#pw-paragraph-indent")
                        .ok().flatten().unwrap().dyn_into().unwrap();
                    let val: f64 = el.value().parse().unwrap_or(0.0);
                    let mut fs = font_settings.get();
                    fs.paragraph_indent = if val.abs() < 0.01 { None } else { Some(val) };
                    save(fs);
                }) as Box<dyn FnMut(_)>);
                el.add_event_listener_with_callback("change", handler.as_ref().unchecked_ref()).ok();
                handler.forget();
            }

            if let Ok(Some(el)) = document.query_selector("#pw-heading-indent") {
                let save = save_typography.clone();
                let doc = document.clone();
                let handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                    let el: web_sys::HtmlSelectElement = doc.query_selector("#pw-heading-indent")
                        .ok().flatten().unwrap().dyn_into().unwrap();
                    let val: f64 = el.value().parse().unwrap_or(0.0);
                    let mut fs = font_settings.get();
                    fs.heading_indent = if val.abs() < 0.01 { None } else { Some(val) };
                    save(fs);
                }) as Box<dyn FnMut(_)>);
                el.add_event_listener_with_callback("change", handler.as_ref().unchecked_ref()).ok();
                handler.forget();
            }

            listeners_attached.set(true);
        });
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                100,
            )
            .ok();
        closure.forget();
    };

    // Factory closures capture only Copy types (Signals) so they are Copy themselves.
    // This lets the rsx macro use them in multiple for-loops without move issues.
    let open_chapter = move |chapter_id: String| {
        move || do_switch_chapter(active_pane, auto_save_timer_id, save_status, editor_loaded, chapter_title, store, &bid_signal.get(), &chapter_id)
    };

    // Navigate to a feedback item: switch to the chapter editor, open the feedback
    // sidebar, and scroll to the quoted text after content loads.
    let navigate_to_feedback = move |chapter_id: String, selected_text: String, context_block: String| {
        move || {
            // If already viewing this chapter, just scroll directly
            if let BookPane::Editor(ref cid) = active_pane.get() {
                if *cid == chapter_id {
                    scroll_to_text_in_editor(&selected_text, &context_block);
                    show_feedback_sidebar.set(true);
                    return;
                }
            }
            // Otherwise switch chapter and scroll after load
            pending_feedback_scroll.set(Some((selected_text.clone(), context_block.clone())));
            show_feedback_sidebar.set(true);
            do_switch_chapter_inner(
                active_pane, auto_save_timer_id, save_status, editor_loaded,
                chapter_title, store, &bid_signal.get(), &chapter_id,
                Some(pending_feedback_scroll),
            );
        }
    };

    // ── Beta reader actions ────────────────────────────────────────
    let add_beta_link = move || {
        let name = new_beta_reader_name.get();
        if name.trim().is_empty() { return; }
        show_beta_link_modal.set(false);
        let bid = bid_signal.get();
        let max_ch = new_beta_max_chapter.get();
        let pinned = if new_beta_pin_version.get() { Some("HEAD".to_string()) } else { None };
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateBetaLinkRequest { reader_name: name, max_chapter_index: max_ch, pinned_commit: pinned };
            if let Ok(link) = api::post::<_, BetaReaderLink>(
                &format!("/api/books/{}/beta-links", bid), &req,
            ).await {
                beta_links.update(|l| l.insert(0, link));
            }
        });
    };

    let delete_beta_link = move |link_id: String| {
        move || {
            let bid = bid_signal.get();
            let lid = link_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_req::<serde_json::Value>(
                    &format!("/api/books/{}/beta-links/{}", bid, lid),
                ).await.is_ok() {
                    beta_links.update(|l| l.retain(|x| x.id != lid));
                }
            });
        }
    };

    let update_beta_link = move || {
        let link = match editing_beta_link.get() {
            Some(l) => l,
            None => return,
        };
        let name = edit_beta_reader_name.get();
        if name.trim().is_empty() { return; }
        let max_ch = edit_beta_max_chapter.get();
        let is_pinned = edit_beta_pinned.get();
        let was_pinned = link.pinned_commit.is_some();
        let pinned_commit = if is_pinned && !was_pinned {
            Some(Some("HEAD".to_string()))
        } else if !is_pinned && was_pinned {
            Some(None)
        } else {
            None
        };
        let bid = bid_signal.get();
        let lid = link.id.clone();
        editing_beta_link.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = UpdateBetaLinkRequest {
                reader_name: Some(name.clone()),
                max_chapter_index: Some(max_ch),
                active: None,
                pinned_commit,
            };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/beta-links/{}", bid, lid), &req,
            ).await.is_ok() {
                // Refresh links to get resolved pinned_commit hash
                if let Ok(links) = api::get::<Vec<BetaReaderLink>>(
                    &format!("/api/books/{}/beta-links", bid),
                ).await {
                    beta_links.set(links);
                }
            }
        });
    };

    let toggle_beta_link_active = move |link_id: String, currently_active: bool| {
        move || {
            let bid = bid_signal.get();
            let lid = link_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let req = UpdateBetaLinkRequest {
                    reader_name: None,
                    max_chapter_index: None,
                    active: Some(!currently_active),
                    pinned_commit: None,
                };
                if api::put::<_, serde_json::Value>(
                    &format!("/api/books/{}/beta-links/{}", bid, lid), &req,
                ).await.is_ok() {
                    beta_links.update(|l| {
                        if let Some(link) = l.iter_mut().find(|x| x.id == lid) {
                            link.active = !currently_active;
                        }
                    });
                }
            });
        }
    };

    let resolve_feedback = move |feedback_id: String| {
        move || {
            let bid = bid_signal.get();
            let fid = feedback_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::put::<_, serde_json::Value>(
                    &format!("/api/books/{}/feedback/{}/resolve", bid, fid),
                    &serde_json::json!({}),
                ).await.is_ok() {
                    beta_feedback.update(|fb| {
                        if let Some(f) = fb.iter_mut().find(|x| x.id == fid) {
                            f.resolved = !f.resolved;
                        }
                    });
                }
            });
        }
    };

    let delete_feedback = move |feedback_id: String| {
        move || {
            let bid = bid_signal.get();
            let fid = feedback_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if api::delete_req::<serde_json::Value>(
                    &format!("/api/books/{}/feedback/{}", bid, fid),
                ).await.is_ok() {
                    beta_feedback.update(|fb| fb.retain(|x| x.id != fid));
                }
            });
        }
    };

    let author_reply = move |feedback_id: String| {
        move || {
            let doc = web_sys::window().unwrap().document().unwrap();
            let selector = format!("#author-reply-{}", feedback_id);
            let input: web_sys::HtmlTextAreaElement = match doc.query_selector(&selector).ok().flatten() {
                Some(el) => el.dyn_into().unwrap(),
                None => return,
            };
            let content = input.value();
            if content.trim().is_empty() { return; }
            input.set_value("");
            let bid = bid_signal.get();
            let fid = feedback_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let req = CreateBetaReplyRequest { content };
                if api::post::<_, serde_json::Value>(
                    &format!("/api/books/{}/feedback/{}/replies", bid, fid), &req,
                ).await.is_ok() {
                    // Refresh feedback
                    if let Ok(fb) = api::get::<Vec<BetaFeedback>>(&format!("/api/books/{}/feedback", bid)).await {
                        beta_feedback.set(fb);
                    }
                }
            });
        }
    };

    let copy_beta_link = move |token: String| {
        move || {
            let origin = web_sys::window()
                .and_then(|w| w.location().origin().ok())
                .unwrap_or_default();
            let url = format!("{}/read/{}", origin, token);
            if let Some(window) = web_sys::window() {
                if let Ok(clipboard) = js_sys::Reflect::get(&window.navigator(), &"clipboard".into()) {
                    let clipboard: web_sys::Clipboard = clipboard.unchecked_into();
                    let _ = clipboard.write_text(&url);
                }
            }
        }
    };

    // ── Sidebar click handlers ──────────────────────────────────
    let _open_chapters_pane = move || {
        active_pane.set(BookPane::Chapters);
        store.sidebar_open.set(false);
    };

    let open_typography_pane = {
        let setup = setup_font_pickers.clone();
        move || {
            active_pane.set(BookPane::Typography);
            listeners_attached.set(false);
            setup();
            store.sidebar_open.set(false);
        }
    };

    let open_beta_pane = move || {
        active_pane.set(BookPane::BetaReaders);
        store.sidebar_open.set(false);
    };

    let go_back_to_chapters = move || {
        active_pane.set(BookPane::Chapters);
    };

    rsx! {
        Fragment {
            style { {TYPOGRAPHY_CSS} }
            style { {BOOK_WORKSPACE_CSS} }
            style { {editor_utils::EDITOR_CSS} }
            // Editor font styles
            style {
                {move || {
                    let fs = store.current_book.get()
                        .and_then(|b| b.font_settings.clone())
                        .unwrap_or_default();

                    let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
                    let h2 = fs.h2.as_deref().unwrap_or("Macondo Swash Caps");
                    let h3 = fs.h3.as_deref().unwrap_or("Macondo Swash Caps");
                    let body = fs.body.as_deref().unwrap_or("Playwrite DE Grund");
                    let quote = fs.quote.as_deref().unwrap_or("inherit");
                    let code = fs.code.as_deref().unwrap_or("monospace");
                    let p_spacing = fs.paragraph_spacing.unwrap_or(8.0);
                    let p_indent = fs.paragraph_indent.unwrap_or(0.0);
                    let h_indent = fs.heading_indent.unwrap_or(0.0);

                    format!(
                        ".book-workspace {{ --rinch-font-family: '{body}', serif; font-family: '{body}', serif; }}
                         .book-workspace h1, .book-workspace h2,
                         .book-workspace h3, .book-workspace h4,
                         .book-workspace h5, .book-workspace h6,
                         .book-workspace .rinch-title,
                         .book-workspace .book-sidebar-title {{ font-family: '{h1}', cursive; }}
                         .book-workspace .chapter-item .rinch-text,
                         .book-workspace .editor-topbar .rinch-text {{ font-family: '{h1}', cursive; }}
                         .book-workspace .sidebar-chapter-item,
                         .book-workspace .chapter-item {{ font-family: '{body}', serif; }}
                         .editor-content {{ font-family: '{body}', serif; }}
                         .editor-content p {{ margin: 0 0 {p_spacing}px 0; text-indent: {p_indent}px; }}
                         .editor-content h1 {{ font-family: '{h1}', cursive; text-indent: {h_indent}px; }}
                         .editor-content h2 {{ font-family: '{h2}', cursive; text-indent: {h_indent}px; }}
                         .editor-content h3, .editor-content h4,
                         .editor-content h5, .editor-content h6 {{ font-family: '{h3}', cursive; text-indent: {h_indent}px; }}
                         .editor-content blockquote {{ font-family: '{quote}', serif; }}
                         .editor-content code, .editor-content pre {{ font-family: '{code}', monospace; }}"
                    )
                }}
            }

            div { class: "book-workspace",
                // ── Backdrop for mobile sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "sidebar-backdrop open" } else { "sidebar-backdrop" }},
                    onclick: move || store.sidebar_open.set(false),
                }

                // ── Sidebar ──
                div {
                    class: {move || if store.sidebar_open.get() { "book-sidebar open" } else { "book-sidebar" }},

                    // Book title
                    div { class: "book-sidebar-title",
                        {move || store.current_book.get().map(|b| b.title.clone()).unwrap_or_default()}
                    }
                    Space { h: "sm" }

                    div { class: "book-sidebar-nav",
                        // Chapters section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Chapters) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            div {
                                style: "display: flex; align-items: center; gap: 4px; cursor: pointer;",
                                onclick: move || chapters_collapsed.update(|c| *c = !*c),
                                {move || if chapters_collapsed.get() { "\u{25b8}" } else { "\u{25be}" }}
                                "Chapters"
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 2px;",
                                ActionIcon {
                                    variant: "subtle",
                                    size: "xs",
                                    onclick: move || {
                                        new_chapter_title.set(String::new());
                                        show_chapter_modal.set(true);
                                    },
                                    {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                                }
                            }
                        }

                        // Chapter list — collapsible
                        div {
                            class: "sidebar-chapter-list",
                            style: {move || if chapters_collapsed.get() { "display: none;" } else { "" }},
                            for chapter in store.chapters.get() {
                                {sidebar_chapter_item(
                                    __scope,
                                    chapter.id.clone(),
                                    chapter.title.clone(),
                                    active_pane,
                                    open_chapter,
                                    move_chapter,
                                )}
                            }
                        }

                        // Typography section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Typography) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            onclick: open_typography_pane,
                            "Typography"
                        }

                        // Beta Readers section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::BetaReaders) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            onclick: open_beta_pane,
                            div {
                                style: "display: flex; align-items: center; justify-content: space-between; width: 100%;",
                                "Beta Readers"
                                if !beta_links.get().is_empty() {
                                    Badge {
                                        variant: "light",
                                        size: "xs",
                                        {move || format!("{}", beta_links.get().len())}
                                    }
                                }
                            }
                        }
                    }

                    // Footer
                    div { class: "book-sidebar-footer",
                        Button {
                            variant: "subtle",
                            size: "xs",
                            onclick: go_dashboard,
                            "\u{2190} Dashboard"
                        }
                        div { class: "book-sidebar-footer-row",
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
                                onclick: toggle_dark,
                                {render_tabler_icon(
                                    __scope,
                                    if store.dark_mode.get() { TablerIcon::Sun } else { TablerIcon::Moon },
                                    TablerIconStyle::Outline,
                                )}
                            }
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
                                onclick: logout,
                                {render_tabler_icon(__scope, TablerIcon::Logout, TablerIconStyle::Outline)}
                            }
                        }
                    }
                }

                // ── Main Pane ──
                div { class: "book-main-pane",
                    // Mobile hamburger bar
                    div { class: "mobile-topbar",
                        ActionIcon {
                            variant: "subtle",
                            onclick: toggle_sidebar,
                            {render_tabler_icon(__scope, TablerIcon::Menu2, TablerIconStyle::Outline)}
                        }
                        if !beta_feedback.get().is_empty() && matches!(active_pane.get(), BookPane::Editor(_)) {
                            ActionIcon {
                                variant: {move || if show_feedback_sidebar.get() { "filled".to_string() } else { "subtle".to_string() }},
                                size: "sm",
                                onclick: move || show_feedback_sidebar.update(|v| *v = !*v),
                                {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
                            }
                        }
                    }

                    // Chapters pane (CSS toggle, always in DOM)
                    div {
                        class: "book-main-scroll",
                        style: {move || if matches!(active_pane.get(), BookPane::Chapters) { "" } else { "display:none;" }},

                        div { class: "chapters-pane",
                            div { class: "chapters-pane-header",
                                Title { order: 3, "Chapters" }
                                Button {
                                    size: "sm",
                                    onclick: move || {
                                        new_chapter_title.set(String::new());
                                        show_chapter_modal.set(true);
                                    },
                                    "Add Chapter"
                                }
                            }

                            if store.chapters.get().is_empty() {
                                Center {
                                    style: "padding: 40px 0;",
                                    Text { color: "dimmed", "No chapters yet. Add one to start writing!" }
                                }
                            }

                            div { class: "chapter-list",
                                for (i, chapter) in store.chapters.get().into_iter().enumerate() {
                                    Paper {
                                        key: chapter.id.clone(),
                                        shadow: "xs",
                                        p: "md",
                                        radius: "sm",
                                        class: "chapter-item",

                                        div {
                                            class: "chapter-item-content",
                                            div {
                                                class: "chapter-item-left",
                                                onclick: open_chapter(chapter.id.clone()),

                                                Badge {
                                                    variant: "light",
                                                    size: "sm",
                                                    {format!("{}", i + 1)}
                                                }
                                                Text { weight: "500", {chapter.title.clone()} }
                                            }
                                            div {
                                                class: "chapter-item-actions",
                                                ActionIcon {
                                                    variant: "subtle",
                                                    size: "sm",
                                                    disabled: i == 0,
                                                    onclick: move_chapter(chapter.id.clone(), -1),
                                                    {render_tabler_icon(
                                                        __scope,
                                                        TablerIcon::ChevronUp,
                                                        TablerIconStyle::Outline,
                                                    )}
                                                }
                                                ActionIcon {
                                                    variant: "subtle",
                                                    size: "sm",
                                                    disabled: i == store.chapters.get().len() - 1,
                                                    onclick: move_chapter(chapter.id.clone(), 1),
                                                    {render_tabler_icon(
                                                        __scope,
                                                        TablerIcon::ChevronDown,
                                                        TablerIconStyle::Outline,
                                                    )}
                                                }
                                                ActionIcon {
                                                    variant: "subtle",
                                                    color: "red",
                                                    size: "sm",
                                                    onclick: delete_chapter(chapter.id.clone()),
                                                    {render_tabler_icon(
                                                        __scope,
                                                        TablerIcon::Trash,
                                                        TablerIconStyle::Outline,
                                                    )}
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Editor pane (CSS toggle, always in DOM — preserves undo history)
                    div {
                        class: "editor-layout",
                        style: {move || if matches!(active_pane.get(), BookPane::Editor(_)) { "" } else { "display:none;" }},

                        div { class: "editor-topbar",
                            div { class: "editor-topbar-left",
                                ActionIcon {
                                    variant: "subtle",
                                    onclick: go_back_to_chapters,
                                    {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                                }
                                Text { weight: "600", {|| chapter_title.get()} }
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 8px;",
                                div {
                                    class: {|| format!("save-indicator {}", save_status.get())},
                                    {|| match save_status.get() {
                                        "saving" => "Saving...".to_string(),
                                        "saved" => "Saved".to_string(),
                                        _ => "Unsaved".to_string(),
                                    }}
                                }
                                if !beta_feedback.get().is_empty() {
                                    ActionIcon {
                                        variant: {move || if show_feedback_sidebar.get() { "filled".to_string() } else { "subtle".to_string() }},
                                        size: "sm",
                                        onclick: move || show_feedback_sidebar.update(|v| *v = !*v),
                                        {render_tabler_icon(__scope, TablerIcon::MessageCircle, TablerIconStyle::Outline)}
                                    }
                                }
                            }
                        }

                        {editor_utils::editor_toolbar(__scope)}

                        div {
                            style: "display: flex; flex: 1; overflow: hidden;",
                            div { class: "editor-scroll",
                                div {
                                    contenteditable: "true",
                                    class: "editor-content",
                                    id: "editor-main",
                                    p { "Start writing..." }
                                }
                            }

                            // Editor feedback backdrop (mobile)
                            div {
                                class: {move || if show_feedback_sidebar.get() { "editor-feedback-backdrop open" } else { "editor-feedback-backdrop" }},
                                onclick: move || show_feedback_sidebar.set(false),
                            }

                            // Feedback sidebar (in editor)
                            div {
                                class: {move || if show_feedback_sidebar.get() { "editor-feedback-sidebar visible" } else { "editor-feedback-sidebar hidden" }},

                                div { class: "editor-feedback-header",
                                    "Feedback"
                                    ActionIcon {
                                        variant: "subtle",
                                        size: "xs",
                                        onclick: move || show_feedback_sidebar.set(false),
                                        {render_tabler_icon(__scope, TablerIcon::X, TablerIconStyle::Outline)}
                                    }
                                }
                                div { class: "editor-feedback-list",
                                    for fb in beta_feedback.get().into_iter().filter(|f| {
                                        if let BookPane::Editor(ref cid) = active_pane.get() {
                                            f.chapter_id == *cid
                                        } else {
                                            false
                                        }
                                    }) {
                                        {editor_feedback_item(__scope, fb, pending_feedback_scroll, author_reply, resolve_feedback, delete_feedback)}
                                    }

                                    if beta_feedback.get().iter().filter(|f| {
                                        if let BookPane::Editor(ref cid) = active_pane.get() {
                                            f.chapter_id == *cid
                                        } else {
                                            false
                                        }
                                    }).count() == 0 {
                                        Center {
                                            style: "padding: 20px 0;",
                                            Text { color: "dimmed", size: "sm", "No feedback for this chapter." }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Typography pane (CSS toggle)
                    div {
                        class: "book-main-scroll",
                        style: {move || if matches!(active_pane.get(), BookPane::Typography) { "" } else { "display:none;" }},

                        div { class: "chapters-pane",
                            Title { order: 3, "Typography" }
                            Space { h: "md" }
                            div { class: "typography-section",
                                Text { weight: "600", size: "sm", "Fonts" }
                                Space { h: "xs" }
                                div { class: "font-selector-grid", id: "font-selector-grid" }
                                Space { h: "lg" }
                                Text { weight: "600", size: "sm", "Spacing" }
                                Space { h: "xs" }
                                div { class: "font-selector-grid", id: "spacing-selector-grid" }
                                Space { h: "lg" }
                                Text { size: "sm", color: "dimmed", "Preview:" }
                                div { class: "font-preview-box", id: "font-preview" }
                            }
                        }
                    }

                    // Beta Readers pane (CSS toggle)
                    div {
                        class: "book-main-scroll",
                        style: {move || if matches!(active_pane.get(), BookPane::BetaReaders) { "" } else { "display:none;" }},

                        div { class: "chapters-pane",
                            div { class: "chapters-pane-header",
                                Title { order: 3, "Beta Readers" }
                                Button {
                                    size: "sm",
                                    onclick: move || {
                                        new_beta_reader_name.set(String::new());
                                        new_beta_max_chapter.set(None);
                                        new_beta_pin_version.set(false);
                                        show_beta_link_modal.set(true);
                                    },
                                    "Create Link"
                                }
                            }

                            Space { h: "sm" }
                            Text { size: "sm", color: "dimmed",
                                "Share these links with your beta readers. They can read and leave feedback without needing an account."
                            }
                            Space { h: "md" }

                            if beta_links.get().is_empty() {
                                Center {
                                    style: "padding: 40px 0;",
                                    Text { color: "dimmed", "No beta reader links yet." }
                                }
                            }

                            div { class: "beta-link-list",
                                for link in beta_links.get() {
                                    {render_beta_link_card(
                                        __scope,
                                        link,
                                        edit_beta_reader_name,
                                        edit_beta_max_chapter,
                                        edit_beta_pinned,
                                        editing_beta_link,
                                        copy_beta_link,
                                        toggle_beta_link_active,
                                        delete_beta_link,
                                    )}
                                }
                            }

                            // Feedback overview section
                            if !beta_feedback.get().is_empty() {
                                Space { h: "lg" }
                                Title { order: 4, "All Feedback" }
                                Space { h: "sm" }

                                div { class: "beta-feedback-overview",
                                    for fb in beta_feedback.get().into_iter().map(|fb| {
                                        let ch_title = store.chapters.get().iter()
                                            .find(|c| c.id == fb.chapter_id)
                                            .map(|c| c.title.clone())
                                            .unwrap_or_else(|| String::from("Unknown"));
                                        (fb, ch_title)
                                    }) {
                                        {overview_feedback_item(__scope, fb.0, fb.1, author_reply, resolve_feedback, delete_feedback, navigate_to_feedback)}
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Add Chapter Modal
            Modal {
                opened_fn: move || show_chapter_modal.get(),
                onclose: move || show_chapter_modal.set(false),
                title: "Add Chapter",

                TextInput {
                    label: "Chapter Title",
                    placeholder: "Enter chapter title",
                    value_fn: move || new_chapter_title.get(),
                    oninput: move |v: String| new_chapter_title.set(v),
                    onsubmit: add_chapter,
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_chapter_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: add_chapter,
                        "Add"
                    }
                }
            }

            // Create Beta Link Modal
            Modal {
                opened_fn: move || show_beta_link_modal.get(),
                onclose: move || show_beta_link_modal.set(false),
                title: "Create Beta Reader Link",

                TextInput {
                    label: "Reader Name",
                    placeholder: "e.g. Alice, Book Club, etc.",
                    value_fn: move || new_beta_reader_name.get(),
                    oninput: move |v: String| new_beta_reader_name.set(v),
                    onsubmit: add_beta_link,
                }
                Space { h: "md" }
                Text { size: "sm", color: "dimmed",
                    "Optionally restrict how many chapters the reader can access:"
                }
                Space { h: "xs" }
                div {
                    style: "display: flex; align-items: center; gap: 8px;",
                    Text { size: "sm", "Max chapters:" }
                    input {
                        id: "beta-max-chapter-input",
                        r#type: "number",
                        min: "1",
                        placeholder: "All",
                        style: "width: 80px; padding: 4px 8px; border: 1px solid var(--rinch-color-border); border-radius: 4px; background: var(--rinch-color-surface); color: var(--rinch-color-text); font-size: 13px;",
                    }
                }
                Space { h: "md" }
                div {
                    style: "display: flex; align-items: center; gap: 8px; cursor: pointer;",
                    onclick: move || new_beta_pin_version.update(|v| *v = !*v),
                    div {
                        style: "width: 20px; height: 20px; border: 1px solid var(--rinch-color-border); border-radius: 3px; display: flex; align-items: center; justify-content: center; font-size: 14px;",
                        {move || if new_beta_pin_version.get() { "\u{2713}" } else { "" }}
                    }
                    Text { size: "sm", "Pin to current version" }
                }
                Text { size: "xs", color: "dimmed",
                    "If pinned, the reader sees a snapshot of the book as it is now. Otherwise, they always see the latest version."
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_beta_link_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: move || {
                            // Read max chapter input
                            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                                if let Ok(Some(el)) = doc.query_selector("#beta-max-chapter-input") {
                                    let input: web_sys::HtmlInputElement = el.dyn_into().unwrap();
                                    let val = input.value();
                                    if let Ok(n) = val.parse::<i64>() {
                                        new_beta_max_chapter.set(Some(n - 1)); // 0-indexed
                                    } else {
                                        new_beta_max_chapter.set(None);
                                    }
                                }
                            }
                            add_beta_link();
                        },
                        "Create"
                    }
                }
            }

            // Edit Beta Link Modal
            Modal {
                opened_fn: move || editing_beta_link.get().is_some(),
                onclose: move || editing_beta_link.set(None),
                title: "Edit Beta Reader Link",

                TextInput {
                    label: "Reader Name",
                    placeholder: "e.g. Alice, Book Club, etc.",
                    value_fn: move || edit_beta_reader_name.get(),
                    oninput: move |v: String| edit_beta_reader_name.set(v),
                    onsubmit: update_beta_link,
                }
                Space { h: "md" }
                Text { size: "sm", color: "dimmed",
                    "Restrict how many chapters the reader can access (leave empty for all):"
                }
                Space { h: "xs" }
                div {
                    style: "display: flex; align-items: center; gap: 8px;",
                    Text { size: "sm", "Max chapters:" }
                    input {
                        id: "beta-edit-max-chapter-input",
                        r#type: "number",
                        min: "1",
                        placeholder: "All",
                        value: {move || edit_beta_max_chapter.get().map(|v| (v + 1).to_string()).unwrap_or_default()},
                        style: "width: 80px; padding: 4px 8px; border: 1px solid var(--rinch-color-border); border-radius: 4px; background: var(--rinch-color-surface); color: var(--rinch-color-text); font-size: 13px;",
                    }
                }
                Space { h: "md" }
                div {
                    style: "display: flex; align-items: center; gap: 8px; cursor: pointer;",
                    onclick: move || edit_beta_pinned.update(|v| *v = !*v),
                    div {
                        style: "width: 20px; height: 20px; border: 1px solid var(--rinch-color-border); border-radius: 3px; display: flex; align-items: center; justify-content: center; font-size: 14px;",
                        {move || if edit_beta_pinned.get() { "\u{2713}" } else { "" }}
                    }
                    Text { size: "sm", "Pin to current version" }
                }
                Text { size: "xs", color: "dimmed",
                    "If pinned, the reader sees a snapshot. Unpin to show the latest version."
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || editing_beta_link.set(None),
                        "Cancel"
                    }
                    Button {
                        onclick: move || {
                            // Read max chapter input
                            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                                if let Ok(Some(el)) = doc.query_selector("#beta-edit-max-chapter-input") {
                                    let input: web_sys::HtmlInputElement = el.dyn_into().unwrap();
                                    let val = input.value();
                                    if let Ok(n) = val.parse::<i64>() {
                                        edit_beta_max_chapter.set(Some(n - 1)); // 0-indexed
                                    } else {
                                        edit_beta_max_chapter.set(None);
                                    }
                                }
                            }
                            update_beta_link();
                        },
                        "Save"
                    }
                }
            }
        }
    }
}
