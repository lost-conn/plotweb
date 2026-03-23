use std::cell::Cell;
use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{
    BetaFeedback, BetaReaderLink, Book, Chapter, CreateBetaLinkRequest,
    CreateBetaReplyRequest, CreateChapterRequest, CreateNoteRequest, FontSettings,
    ImportChapter, ImportPreviewChapter, ImportPreviewResponse, MoveNoteRequest,
    Note, NoteTree, NotesResponse, ReorderChaptersRequest, UpdateBetaLinkRequest,
    UpdateBookRequest, UpdateChapterRequest, UpdateNoteRequest, UpdateNoteTreeRequest,
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
    Notes,
    NoteEditor(String),
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

/// CSS for the notes tree.
const NOTES_CSS: &str = r#"
.notes-pane {
    padding: 0;
    overflow-x: auto;
    overflow-y: auto;
    height: 100%;
}
.notes-pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 24px 24px 0 24px;
}
.notes-tree {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px 24px 24px;
    min-width: min-content;
}
.notes-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 60px 24px;
}
.note-branch {
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: 0;
}
.note-card {
    width: 220px;
    min-height: 48px;
    flex-shrink: 0;
    padding: 8px 10px;
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-left: 3px solid var(--note-color, var(--rinch-color-teal-6));
    cursor: grab;
    transition: box-shadow 0.15s, border-color 0.15s;
    position: relative;
}
.note-card:hover {
    box-shadow: 0 2px 8px rgba(0,0,0,0.15);
}
.note-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
}
.note-card-title {
    font-size: 13px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    color: var(--rinch-color-text);
}
.note-card-actions {
    display: flex;
    align-items: center;
    gap: 0;
    opacity: 0;
    transition: opacity 0.15s;
}
.note-card:hover .note-card-actions {
    opacity: 1;
}
.note-card-preview {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
    margin-top: 4px;
    max-height: 80px;
    overflow: hidden;
    line-height: 1.4;
    white-space: pre-wrap;
    word-break: break-word;
}
.note-children {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-left: 16px;
    position: relative;
    justify-content: center;
}
.note-children::before {
    content: '';
    position: absolute;
    left: 0;
    top: 20px;
    bottom: 20px;
    width: 1px;
    background: var(--rinch-color-border);
}
.note-child-row {
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    position: relative;
}
.note-child-row::before {
    content: '';
    position: absolute;
    left: -16px;
    width: 16px;
    height: 1px;
    background: var(--rinch-color-border);
    top: 20px;
}
.note-collapse-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border: none;
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-radius: 50%;
    cursor: pointer;
    font-size: 9px;
    color: var(--rinch-color-dimmed);
    position: absolute;
    right: -9px;
    top: 50%;
    transform: translateY(-50%);
    z-index: 1;
    line-height: 1;
}
.note-collapse-btn:hover {
    background: var(--rinch-color-border);
}
.note-drop-zone {
    height: 0;
    border-radius: 2px;
    transition: height 0.1s, background 0.1s;
    flex-shrink: 0;
}
.note-drop-zone.visible {
    height: 8px;
}
.note-drop-zone.active {
    height: 8px;
    background: var(--rinch-color-teal-6);
}
.note-card.dragging {
    opacity: 0.4;
    pointer-events: none;
}
.note-card.drop-child {
    border-color: var(--rinch-color-teal-6);
    box-shadow: 0 0 0 2px var(--rinch-color-teal-6);
}
.note-editor-pane {
    display: flex;
    flex-direction: column;
    height: 100%;
}
.note-editor-topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    border-bottom: 1px solid var(--rinch-color-border);
    flex-shrink: 0;
}
.note-editor-topbar-left {
    display: flex;
    align-items: center;
    gap: 8px;
}
.note-editor-body {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
}
.note-color-picker {
    display: flex;
    gap: 6px;
    align-items: center;
}
.note-color-dot {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    cursor: pointer;
    border: 2px solid transparent;
    transition: border-color 0.15s, transform 0.15s;
}
.note-color-dot:hover {
    transform: scale(1.15);
}
.note-color-dot.selected {
    border-color: var(--rinch-color-text);
}
.note-save-indicator {
    font-size: 12px;
    color: var(--rinch-color-dimmed);
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
    display: flex;
    align-items: center;
    gap: 4px;
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

/* ── Inline editor title input ────────────────────── */

.editor-title-input {
    flex: 1;
    min-width: 0;
}

.editor-title-input input {
    border: none !important;
    background: transparent !important;
    box-shadow: none !important;
    font-weight: 600;
    font-size: 16px;
    padding: 2px 4px;
    color: var(--rinch-color-text);
    border-bottom: 2px solid transparent !important;
    border-radius: 0 !important;
    transition: border-color 0.15s;
}

.editor-title-input input:focus {
    border-bottom-color: var(--rinch-color-teal-6) !important;
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

    // Collect all text content with node boundaries.
    // Insert a single space at block-element boundaries so that cross-paragraph
    // selections (which include \n between blocks) can still be matched after
    // we normalize whitespace in both the needle and the haystack.
    let mut text_nodes: Vec<web_sys::Node> = Vec::new();
    let mut full_text = String::new();
    let mut node_offsets: Vec<(usize, usize)> = Vec::new(); // (start_byte, end_byte) in full_text

    collect_text_nodes_spaced(&editor.into(), &mut text_nodes, &mut full_text, &mut node_offsets);

    // Normalize whitespace: collapse runs of whitespace into a single space.
    // Build a mapping from normalized byte offset back to original byte offset.
    let (norm_text, norm_to_orig) = normalize_whitespace_map(&full_text);

    // Normalize the needle the same way
    let norm_needle = normalize_whitespace(selected_text);
    if norm_needle.is_empty() {
        return;
    }

    // Find all matches of the normalized needle in the normalized text
    let mut matches: Vec<usize> = Vec::new();
    let mut search_start = 0;
    while let Some(pos) = norm_text[search_start..].find(&norm_needle) {
        matches.push(search_start + pos);
        search_start += pos + 1;
    }

    // Fuzzy fallback: if no exact match, try progressively shorter prefixes
    // (the author may have edited the text after feedback was left)
    if matches.is_empty() && norm_needle.len() > 20 {
        let min_len = norm_needle.len() / 2;
        let mut try_len = norm_needle.len() - 1;
        while try_len >= min_len {
            // Ensure we don't cut in the middle of a UTF-8 char
            while try_len > 0 && !norm_needle.is_char_boundary(try_len) {
                try_len -= 1;
            }
            if try_len == 0 { break; }
            let prefix = &norm_needle[..try_len];
            search_start = 0;
            while let Some(pos) = norm_text[search_start..].find(prefix) {
                matches.push(search_start + pos);
                search_start += pos + 1;
            }
            if !matches.is_empty() {
                log::info!("scroll_to_text: fuzzy match with {}/{} chars", try_len, norm_needle.len());
                break;
            }
            try_len -= 1;
        }
    }

    if matches.is_empty() {
        log::warn!("scroll_to_text: '{}' not found in editor content ({} chars)", &selected_text[..selected_text.len().min(40)], full_text.len());
        return;
    }
    log::info!("scroll_to_text: found {} match(es)", matches.len());

    // Determine the effective needle length used (may be shorter if fuzzy matched)
    let effective_needle_len = if matches.is_empty() { norm_needle.len() } else {
        // Check if first match is an exact-length match
        let first = matches[0];
        if first + norm_needle.len() <= norm_text.len() && &norm_text[first..first + norm_needle.len()] == norm_needle.as_str() {
            norm_needle.len()
        } else {
            // Fuzzy: find the prefix length that matched
            let mut l = norm_needle.len() - 1;
            while l > 0 {
                if norm_needle.is_char_boundary(l) && norm_text[first..].starts_with(&norm_needle[..l]) {
                    break;
                }
                l -= 1;
            }
            l
        }
    };

    // Pick the best match using context_block for disambiguation
    let norm_context = normalize_whitespace(context_block);
    let norm_match_start = if matches.len() == 1 || norm_context.is_empty() {
        matches[0]
    } else {
        // Find the match whose surrounding text best matches context_block
        // by looking for the longest common substring between context and surrounding text
        *matches.iter().min_by_key(|&&pos| {
            let ctx_start = pos.saturating_sub(norm_context.len());
            let ctx_end = (pos + effective_needle_len + norm_context.len()).min(norm_text.len());
            let surrounding = &norm_text[ctx_start..ctx_end];
            // Score: length of longest contiguous overlap with context
            let score = longest_common_substring(surrounding, &norm_context);
            norm_context.len().saturating_sub(score)
        }).unwrap()
    };

    // Map normalized offsets back to original byte offsets
    let match_start = norm_to_orig[norm_match_start];
    let match_end = if norm_match_start + effective_needle_len < norm_to_orig.len() {
        norm_to_orig[norm_match_start + effective_needle_len]
    } else {
        full_text.len()
    };

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

/// Recursively collect text nodes under `node`, inserting a space at block-element
/// boundaries so that cross-paragraph selections can be matched.
fn collect_text_nodes_spaced(
    node: &web_sys::Node,
    text_nodes: &mut Vec<web_sys::Node>,
    full_text: &mut String,
    node_offsets: &mut Vec<(usize, usize)>,
) {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        let start = full_text.len();
        let content = node.text_content().unwrap_or_default();
        full_text.push_str(&content);
        node_offsets.push((start, full_text.len()));
        text_nodes.push(node.clone());
        return;
    }
    // Insert a space before block elements to separate their text
    let is_block = if let Ok(el) = node.clone().dyn_into::<web_sys::Element>() {
        let tag = el.tag_name().to_lowercase();
        matches!(tag.as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li" | "br")
    } else {
        false
    };
    if is_block && !full_text.is_empty() && !full_text.ends_with(' ') {
        full_text.push(' ');
    }
    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.item(i) {
            collect_text_nodes_spaced(&child, text_nodes, full_text, node_offsets);
        }
    }
}

/// Collapse runs of whitespace into a single space and trim.
fn normalize_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut last_was_space = true; // true to trim leading
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    if result.ends_with(' ') {
        result.pop();
    }
    result
}

/// Normalize whitespace and return a mapping from each normalized byte offset
/// to the corresponding original byte offset.
fn normalize_whitespace_map(s: &str) -> (String, Vec<usize>) {
    let mut result = String::with_capacity(s.len());
    let mut mapping = Vec::with_capacity(s.len());
    let mut last_was_space = true;
    for (byte_idx, c) in s.char_indices() {
        if c.is_whitespace() {
            if !last_was_space {
                mapping.push(byte_idx);
                result.push(' ');
                last_was_space = true;
            }
        } else {
            // For multi-byte chars, push the byte_idx for each byte of the output char
            let start = result.len();
            result.push(c);
            let added = result.len() - start;
            for i in 0..added {
                mapping.push(byte_idx + i);
            }
            last_was_space = false;
        }
    }
    if result.ends_with(' ') {
        result.pop();
        mapping.pop();
    }
    // Sentinel: map one past the end
    mapping.push(s.len());
    (result, mapping)
}

/// Find the length of the longest common substring between two strings.
fn longest_common_substring(a: &str, b: &str) -> usize {
    // Simple O(n*m) approach, fine for short context blocks (<= 200 chars)
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut max_len = 0;
    // Use a single-row DP to save memory
    let mut prev = vec![0usize; b_bytes.len() + 1];
    let mut curr = vec![0usize; b_bytes.len() + 1];
    for i in 1..=a_bytes.len() {
        for j in 1..=b_bytes.len() {
            if a_bytes[i - 1] == b_bytes[j - 1] {
                curr[j] = prev[j - 1] + 1;
                if curr[j] > max_len {
                    max_len = curr[j];
                }
            } else {
                curr[j] = 0;
            }
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.iter_mut().for_each(|v| *v = 0);
    }
    max_len
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
    chapter_title_save_timer_id: Signal<i32>,
    save_status: Signal<&'static str>,
    editor_loaded: Signal<bool>,
    chapter_title: Signal<String>,
    store: AppStore,
    bid: &str,
    new_chapter_id: &str,
) {
    do_switch_chapter_inner(active_pane, auto_save_timer_id, chapter_title_save_timer_id, save_status, editor_loaded, chapter_title, store, bid, new_chapter_id, None);
}

thread_local! {
    static SWITCH_GEN: Cell<u32> = Cell::new(0);
    /// Incremented each time `book_page()` is created so that leaked timer
    /// closures from a previous page instance can detect they are stale.
    static PAGE_GEN: Cell<u32> = Cell::new(0);
}

fn do_switch_chapter_inner(
    active_pane: Signal<BookPane>,
    auto_save_timer_id: Signal<i32>,
    chapter_title_save_timer_id: Signal<i32>,
    save_status: Signal<&'static str>,
    editor_loaded: Signal<bool>,
    chapter_title: Signal<String>,
    store: AppStore,
    bid: &str,
    new_chapter_id: &str,
    pending_scroll: Option<Signal<Option<(String, String)>>>,
) {
    // Increment switch generation to cancel any in-flight loads
    SWITCH_GEN.with(|g| g.set(g.get().wrapping_add(1)));
    let switch_gen = SWITCH_GEN.with(|g| g.get());

    // Clear any pending auto-save timer
    let prev = auto_save_timer_id.get();
    if prev != 0 {
        if let Some(w) = web_sys::window() {
            w.clear_timeout_with_handle(prev);
        }
        auto_save_timer_id.set(0);
    }

    // Clear any pending chapter-title save timer
    let prev_title = chapter_title_save_timer_id.get();
    if prev_title != 0 {
        if let Some(w) = web_sys::window() {
            w.clear_timeout_with_handle(prev_title);
        }
        chapter_title_save_timer_id.set(0);
    }

    // Save current chapter if we're editing — read DOM synchronously to avoid
    // stale content if another switch happens before the async block runs.
    if let BookPane::Editor(ref current_id) = active_pane.get() {
        let current_id = current_id.clone();
        let bid = bid.to_string();
        let content = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector("#editor-main").ok().flatten())
            .map(|el| el.inner_html());
        if let Some(content) = content {
            let markdown = editor_utils::html_to_markdown(&content);
            save_status.set("saving");
            wasm_bindgen_futures::spawn_local(async move {
                let req = UpdateChapterRequest { title: None, content: Some(markdown) };
                if api::put::<_, serde_json::Value>(
                    &format!("/api/books/{}/chapters/{}", bid, current_id),
                    &req,
                ).await.is_ok() {
                    save_status.set("saved");
                }
            });
        }
    }

    // Disable editor to prevent typing during the transition window
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector("#editor-main").ok().flatten())
    {
        el.set_attribute("contenteditable", "false").ok();
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
                // Bail if another switch happened since we started
                if SWITCH_GEN.with(|g| g.get()) != switch_gen {
                    return;
                }
                if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                    if let Ok(Some(el)) = doc.query_selector("#editor-main") {
                        if content.is_empty() {
                            el.set_inner_html("<p></p>");
                        } else {
                            el.set_inner_html(&editor_utils::markdown_to_html(&content));
                        }
                        // Re-enable editing now that the new content is loaded
                        el.set_attribute("contenteditable", "true").ok();
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
    edit_beta_username: Signal<String>,
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
    let user_badge: Signal<Option<String>> = Signal::new(link.username.as_ref().map(|u| format!("@{}", u)));
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
                            edit_beta_username.set(edit_link.username.clone().unwrap_or_default());
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
                if let Some(ref ub) = user_badge.get() {
                    Badge { variant: "light", size: "xs", color: "violet", {ub.clone()} }
                }
                if is_pinned {
                    Badge { variant: "light", size: "xs", {pin_badge.clone()} }
                } else {
                    Badge { variant: "outline", size: "xs", "Live" }
                }
            }
        }
    }
}

/// Render a drop zone for reordering notes between siblings.
/// `parent_id` is None for root level, Some(id) for children of a note.
/// `index` is the insertion position among siblings.
fn render_drop_zone(
    __scope: &mut RenderScope,
    parent_id: Signal<Option<String>>,
    index: usize,
    drop_target: Signal<Option<(Option<String>, usize)>>,
    dragging_note_id: Signal<Option<String>>,
) -> NodeHandle {
    rsx! {
        div {
            class: {move || {
                if dragging_note_id.get().is_none() {
                    "note-drop-zone"
                } else {
                    let is_active = drop_target.get()
                        .map(|(pid, idx)| pid == parent_id.get() && idx == index)
                        .unwrap_or(false);
                    if is_active { "note-drop-zone active" } else { "note-drop-zone visible" }
                }
            }},
            ondragenter: move || {
                drop_target.set(Some((parent_id.get(), index)));
            },
        }
    }
}

/// Helper to strip HTML tags and produce a text preview, preserving paragraph breaks.
fn note_content_preview(html: &str) -> String {
    if html.is_empty() {
        return String::new();
    }
    let text = html
        .replace("</p>", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("</div>", "\n")
        .replace("</li>", "\n");
    let mut result = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' { in_tag = true; }
        else if ch == '>' { in_tag = false; }
        else if !in_tag { result.push(ch); }
    }
    let mut cleaned = String::new();
    let mut last_was_newline = false;
    for ch in result.trim().chars() {
        if ch == '\n' {
            if !last_was_newline {
                cleaned.push('\n');
                last_was_newline = true;
            }
        } else {
            cleaned.push(ch);
            last_was_newline = false;
        }
    }
    cleaned.chars().take(200).collect()
}

/// Render a note card and its children recursively as a horizontal tree.
/// All data is read reactively from store signals inside closures.
fn render_note_card(
    __scope: &mut RenderScope,
    note_id: String,
    store: AppStore,
    active_pane: Signal<BookPane>,
    bid_signal: Signal<String>,
    new_note_title: Signal<String>,
    new_note_parent_id: Signal<Option<String>>,
    new_note_color: Signal<String>,
    show_note_modal: Signal<bool>,
    note_editor_title: Signal<String>,
    note_editor_color: Signal<Option<String>>,
    dragging_note_id: Signal<Option<String>>,
    drop_target: Signal<Option<(Option<String>, usize)>>,
) -> NodeHandle {
    let nid = Signal::new(note_id);

    rsx! {
        div { class: "note-branch",
            div {
                style: "position: relative; flex-shrink: 0;",
                div {
                    class: {move || {
                        let mut cls = "note-card".to_string();
                        let n = nid.get();
                        if dragging_note_id.get().as_deref() == Some(n.as_str()) {
                            cls.push_str(" dragging");
                        }
                        if let Some((ref pid, _)) = drop_target.get() {
                            if pid.as_deref() == Some(n.as_str()) {
                                cls.push_str(" drop-child");
                            }
                        }
                        cls
                    }},
                    style: {move || {
                        let notes = store.notes.get();
                        let n = nid.get();
                        let color = notes.iter().find(|note| note.id == n)
                            .and_then(|note| note.color.clone())
                            .unwrap_or_else(|| "teal".to_string());
                        format!("--note-color: var(--rinch-color-{}-6);", color)
                    }},
                    draggable: "true",
                    ondragstart: move || {
                        dragging_note_id.set(Some(nid.get()));
                    },
                    ondragend: move || {
                        if let Some(target) = drop_target.get() {
                            if let Some(drag_id) = dragging_note_id.get() {
                                let bid = bid_signal.get();
                                let req = MoveNoteRequest {
                                    note_id: drag_id,
                                    new_parent_id: target.0,
                                    index: target.1,
                                };
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Ok(_) = api::put::<_, serde_json::Value>(
                                        &format!("/api/books/{}/notes/move", bid), &req,
                                    ).await {
                                        if let Ok(resp) = api::get::<NotesResponse>(
                                            &format!("/api/books/{}/notes", bid),
                                        ).await {
                                            store.notes.set(resp.notes);
                                            store.note_tree.set(Some(resp.tree));
                                        }
                                    }
                                });
                            }
                        }
                        dragging_note_id.set(None);
                        drop_target.set(None);
                    },
                    ondragenter: move || {
                        drop_target.set(Some((Some(nid.get()), 0)));
                    },
                    onclick: move || {
                        let n = nid.get();
                        let bid = bid_signal.get();
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Ok(note) = api::get::<Note>(
                                &format!("/api/books/{}/notes/{}", bid, n),
                            ).await {
                                note_editor_title.set(note.title);
                                note_editor_color.set(note.color);
                                active_pane.set(BookPane::NoteEditor(n.clone()));

                                let content = note.content;
                                let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
                                    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                                        if let Some(el) = doc.get_element_by_id("note-editor-main") {
                                            el.set_inner_html(&content);
                                        }
                                    }
                                }) as Box<dyn FnMut()>);
                                if let Some(w) = web_sys::window() {
                                    w.set_timeout_with_callback_and_timeout_and_arguments_0(
                                        closure.as_ref().unchecked_ref(), 50,
                                    ).ok();
                                }
                                closure.forget();
                            }
                        });
                        store.sidebar_open.set(false);
                    },

                    div { class: "note-card-header",
                        div { class: "note-card-title",
                            {move || {
                                let notes = store.notes.get();
                                let n = nid.get();
                                notes.iter().find(|note| note.id == n)
                                    .map(|note| note.title.clone())
                                    .unwrap_or_default()
                            }}
                        }
                        div { class: "note-card-actions",
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                onclick: move || {
                                    new_note_title.set(String::new());
                                    new_note_parent_id.set(Some(nid.get()));
                                    new_note_color.set("teal".to_string());
                                    show_note_modal.set(true);
                                },
                                {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                            }
                            ActionIcon {
                                variant: "subtle",
                                size: "xs",
                                color: "red",
                                onclick: move || {
                                    let n = nid.get();
                                    let bid = bid_signal.get();
                                    wasm_bindgen_futures::spawn_local(async move {
                                        if let Ok(_) = api::delete_req::<serde_json::Value>(
                                            &format!("/api/books/{}/notes/{}", bid, n),
                                        ).await {
                                            if let Ok(resp) = api::get::<NotesResponse>(
                                                &format!("/api/books/{}/notes", bid),
                                            ).await {
                                                store.notes.set(resp.notes);
                                                store.note_tree.set(Some(resp.tree));
                                            }
                                        }
                                    });
                                },
                                {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
                            }
                        }
                    }
                    div {
                        class: "note-card-preview",
                        {move || {
                            let notes = store.notes.get();
                            let n = nid.get();
                            note_content_preview(
                                &notes.iter().find(|note| note.id == n)
                                    .map(|note| note.content.clone())
                                    .unwrap_or_default()
                            )
                        }}
                    }
                }
                if store.note_tree.get().map(|t| t.children.get(&nid.get()).map(|c| !c.is_empty()).unwrap_or(false)).unwrap_or(false) {
                    button {
                        class: "note-collapse-btn",
                        onclick: move || {
                            if let Some(tree) = store.note_tree.get() {
                                let n = nid.get();
                                let mut new_tree = tree.clone();
                                if new_tree.collapsed.contains(&n) {
                                    new_tree.collapsed.retain(|id| id != &n);
                                } else {
                                    new_tree.collapsed.push(n.clone());
                                }
                                store.note_tree.set(Some(new_tree.clone()));
                                let bid = bid_signal.get();
                                let tree_req = UpdateNoteTreeRequest {
                                    tree: NoteTree {
                                        root_order: new_tree.root_order,
                                        children: new_tree.children,
                                        collapsed: new_tree.collapsed,
                                    },
                                };
                                wasm_bindgen_futures::spawn_local(async move {
                                    api::put::<_, serde_json::Value>(
                                        &format!("/api/books/{}/notes/tree", bid),
                                        &tree_req,
                                    ).await.ok();
                                });
                            }
                        },
                        {move || {
                            if store.note_tree.get().map(|t| t.collapsed.contains(&nid.get())).unwrap_or(false) {
                                "\u{25b8}"
                            } else {
                                "\u{25be}"
                            }
                        }}
                    }
                }
            }

            if store.note_tree.get().map(|t| {
                let n = nid.get();
                let has_children = t.children.get(&n).map(|c| !c.is_empty()).unwrap_or(false);
                let collapsed = t.collapsed.contains(&n);
                has_children && !collapsed
            }).unwrap_or(false) {
                div { class: "note-children",
                    for (idx, child_id) in store.note_tree.get().and_then(|t| t.children.get(&nid.get()).cloned()).unwrap_or_default().into_iter().enumerate() {
                        div { style: "display: contents;",
                            {render_drop_zone(__scope, Signal::new(Some(nid.get())), idx, drop_target, dragging_note_id)}
                            div { class: "note-child-row",
                                {render_note_card(
                                    __scope,
                                    child_id.clone(),
                                    store,
                                    active_pane,
                                    bid_signal,
                                    new_note_title,
                                    new_note_parent_id,
                                    new_note_color,
                                    show_note_modal,
                                    note_editor_title,
                                    note_editor_color,
                                    dragging_note_id,
                                    drop_target,
                                )}
                            }
                        }
                    }
                    {render_drop_zone(__scope, Signal::new(Some(nid.get())), store.note_tree.get().and_then(|t| t.children.get(&nid.get()).map(|c| c.len())).unwrap_or(0), drop_target, dragging_note_id)}
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
    PAGE_GEN.with(|g| g.set(g.get().wrapping_add(1)));
    let page_gen = PAGE_GEN.with(|g| g.get());
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
    let new_beta_username: Signal<String> = Signal::new(String::new());
    let edit_beta_username: Signal<String> = Signal::new(String::new());
    let beta_link_error: Signal<Option<String>> = Signal::new(None);

    // Import signals
    let show_import_modal: Signal<bool> = Signal::new(false);
    let import_preview: Signal<Vec<ImportPreviewChapter>> = Signal::new(Vec::new());
    let import_filename: Signal<String> = Signal::new(String::new());
    let import_loading: Signal<bool> = Signal::new(false);
    let import_error: Signal<Option<String>> = Signal::new(None);
    let import_file: Signal<Option<web_sys::File>> = Signal::new(None);
    // Full chapter content stored separately (preview only has truncated text)
    let import_full_chapters: Signal<Vec<ImportChapter>> = Signal::new(Vec::new());

    // Note signals
    let show_note_modal: Signal<bool> = Signal::new(false);
    let new_note_title: Signal<String> = Signal::new(String::new());
    let new_note_parent_id: Signal<Option<String>> = Signal::new(None);
    let new_note_color: Signal<String> = Signal::new("teal".to_string());
    let note_save_status: Signal<&str> = Signal::new("saved");
    let note_save_timer_id: Signal<i32> = Signal::new(0);
    let note_editor_title: Signal<String> = Signal::new(String::new());
    let note_editor_color: Signal<Option<String>> = Signal::new(None);
    let dragging_note_id: Signal<Option<String>> = Signal::new(None);
    let drop_target: Signal<Option<(Option<String>, usize)>> = Signal::new(None);

    // Book settings modal
    let show_book_settings_modal: Signal<bool> = Signal::new(false);
    let edit_book_title: Signal<String> = Signal::new(String::new());
    let edit_book_desc: Signal<String> = Signal::new(String::new());

    // Chapter rename modal
    let show_rename_chapter_modal: Signal<bool> = Signal::new(false);
    let rename_chapter_id: Signal<String> = Signal::new(String::new());
    let rename_chapter_title: Signal<String> = Signal::new(String::new());

    // Inline chapter title save timer
    let chapter_title_save_timer_id: Signal<i32> = Signal::new(0);

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
        // Bail if this page instance is no longer current (user navigated away and back)
        if PAGE_GEN.with(|g| g.get()) != page_gen {
            return;
        }
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
            // Capture context now; validate it still matches when the timer fires
            let captured_cid = if let BookPane::Editor(ref cid) = active_pane.get() {
                cid.clone()
            } else {
                return;
            };
            let save_fn = save_for_input.clone();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                // Bail if this page instance is stale (user navigated away and back)
                if PAGE_GEN.with(|g| g.get()) != page_gen { return; }
                // Verify we're still on the same chapter
                if let BookPane::Editor(ref current_cid) = active_pane.get() {
                    if *current_cid == captured_cid {
                        save_fn(captured_cid);
                    }
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

    // ── Book settings save ──────────────────────────────────────
    let save_book_settings = move || {
        let title = edit_book_title.get();
        if title.trim().is_empty() {
            return;
        }
        show_book_settings_modal.set(false);
        let bid = bid_signal.get();
        let desc = edit_book_desc.get();
        wasm_bindgen_futures::spawn_local(async move {
            let req = UpdateBookRequest {
                title: Some(title.clone()),
                description: Some(desc.clone()),
                font_settings: None,
            };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}", bid),
                &req,
            ).await.is_ok() {
                store.current_book.update(|book| {
                    if let Some(b) = book {
                        b.title = title;
                        b.description = desc;
                    }
                });
            }
        });
    };

    // ── Chapter rename save ───────────────────────────────────────
    let save_rename_chapter = move || {
        let title = rename_chapter_title.get();
        if title.trim().is_empty() {
            return;
        }
        show_rename_chapter_modal.set(false);
        let bid = bid_signal.get();
        let cid = rename_chapter_id.get();
        wasm_bindgen_futures::spawn_local(async move {
            let req = UpdateChapterRequest {
                title: Some(title.clone()),
                content: None,
            };
            if api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/chapters/{}", bid, cid),
                &req,
            ).await.is_ok() {
                store.chapters.update(|chapters| {
                    if let Some(ch) = chapters.iter_mut().find(|c| c.id == cid) {
                        ch.title = title.clone();
                    }
                });
                // Update editor title if this chapter is currently open
                if let BookPane::Editor(ref open_cid) = active_pane.get() {
                    if *open_cid == cid {
                        chapter_title.set(title);
                    }
                }
            }
        });
    };

    // ── Inline chapter title save (debounced) ─────────────────────
    let save_chapter_title_inline = move |new_title: String| {
        chapter_title.set(new_title.clone());
        let prev = chapter_title_save_timer_id.get();
        if prev != 0 {
            if let Some(w) = web_sys::window() {
                w.clear_timeout_with_handle(prev);
            }
        }
        // Capture context now, validate when timer fires
        let captured_bid = bid_signal.get();
        let captured_cid = if let BookPane::Editor(ref cid) = active_pane.get() {
            cid.clone()
        } else {
            return;
        };
        let save_closure = wasm_bindgen::closure::Closure::once(move || {
            // Bail if this page instance is stale (user navigated away and back)
            if PAGE_GEN.with(|g| g.get()) != page_gen { return; }
            if let BookPane::Editor(ref current_cid) = active_pane.get() {
                if *current_cid != captured_cid { return; }
            } else { return; }
            let cid = captured_cid;
            let title = new_title.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let req = UpdateChapterRequest {
                    title: Some(title.clone()),
                    content: None,
                };
                if api::put::<_, serde_json::Value>(
                    &format!("/api/books/{}/chapters/{}", captured_bid, cid),
                    &req,
                ).await.is_ok() {
                    store.chapters.update(|chapters| {
                        if let Some(ch) = chapters.iter_mut().find(|c| c.id == cid) {
                            ch.title = title;
                        }
                    });
                }
            });
        });
        let id = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                save_closure.as_ref().unchecked_ref(),
                1000,
            )
            .unwrap_or(0);
        save_closure.forget();
        chapter_title_save_timer_id.set(id);
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
        move || do_switch_chapter(active_pane, auto_save_timer_id, chapter_title_save_timer_id, save_status, editor_loaded, chapter_title, store, &bid_signal.get(), &chapter_id)
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
                active_pane, auto_save_timer_id, chapter_title_save_timer_id, save_status, editor_loaded,
                chapter_title, store, &bid_signal.get(), &chapter_id,
                Some(pending_feedback_scroll),
            );
        }
    };

    // ── Beta reader actions ────────────────────────────────────────
    let add_beta_link = move || {
        let name = new_beta_reader_name.get();
        if name.trim().is_empty() { return; }
        beta_link_error.set(None);
        let bid = bid_signal.get();
        let max_ch = new_beta_max_chapter.get();
        let pinned = if new_beta_pin_version.get() { Some("HEAD".to_string()) } else { None };
        let username_val = new_beta_username.get();
        let username = if username_val.trim().is_empty() { None } else { Some(username_val) };
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateBetaLinkRequest { reader_name: name, max_chapter_index: max_ch, pinned_commit: pinned, username };
            match api::post::<_, BetaReaderLink>(
                &format!("/api/books/{}/beta-links", bid), &req,
            ).await {
                Ok(link) => {
                    show_beta_link_modal.set(false);
                    beta_links.update(|l| l.insert(0, link));
                }
                Err(e) => {
                    beta_link_error.set(Some(format!("{}", e)));
                }
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
        let new_username = edit_beta_username.get();
        let old_username = link.username.clone().unwrap_or_default();
        let username = if new_username.trim() != old_username.trim() {
            if new_username.trim().is_empty() {
                Some(None) // Detach
            } else {
                Some(Some(new_username)) // Attach new user
            }
        } else {
            None // No change
        };
        let bid = bid_signal.get();
        let lid = link.id.clone();
        beta_link_error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = UpdateBetaLinkRequest {
                reader_name: Some(name.clone()),
                max_chapter_index: Some(max_ch),
                active: None,
                pinned_commit,
                username,
            };
            match api::put::<_, serde_json::Value>(
                &format!("/api/books/{}/beta-links/{}", bid, lid), &req,
            ).await {
                Ok(_) => {
                    editing_beta_link.set(None);
                    // Refresh links to get resolved data
                    if let Ok(links) = api::get::<Vec<BetaReaderLink>>(
                        &format!("/api/books/{}/beta-links", bid),
                    ).await {
                        beta_links.set(links);
                    }
                }
                Err(e) => {
                    beta_link_error.set(Some(format!("{}", e)));
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
                    username: None,
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

    let open_notes_pane = move || {
        active_pane.set(BookPane::Notes);
        store.sidebar_open.set(false);
        // Fetch notes
        let bid = bid_signal.get();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(resp) = api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid)).await {
                store.notes.set(resp.notes);
                store.note_tree.set(Some(resp.tree));
            }
        });
    };

    let add_note = move || {
        let title = new_note_title.get().trim().to_string();
        if title.is_empty() {
            return;
        }
        let parent_id = new_note_parent_id.get();
        let color = new_note_color.get();
        let bid = bid_signal.get();
        show_note_modal.set(false);
        // Switch to Notes pane so the new note is visible
        active_pane.set(BookPane::Notes);
        wasm_bindgen_futures::spawn_local(async move {
            let req = CreateNoteRequest {
                title,
                parent_id: parent_id.clone(),
                color: Some(color),
            };
            match api::post::<_, Note>(&format!("/api/books/{}/notes", bid), &req).await {
                Ok(_note) => {
                    // Refresh tree
                    if let Ok(resp) = api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid)).await {
                        store.notes.set(resp.notes);
                        store.note_tree.set(Some(resp.tree));
                    }
                }
                Err(e) => eprintln!("Failed to create note: {}", e.message),
            }
        });
    };

    let save_note_content = move || {
        if let BookPane::NoteEditor(ref nid) = active_pane.get() {
            let nid = nid.clone();
            let bid = bid_signal.get();
            let title_val = note_editor_title.get();
            let color_val = note_editor_color.get();

            let content = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("note-editor-main"))
                .map(|el| el.inner_html())
                .unwrap_or_default();

            note_save_status.set("saving");
            wasm_bindgen_futures::spawn_local(async move {
                let req = UpdateNoteRequest {
                    title: Some(title_val),
                    content: Some(content),
                    color: color_val,
                };
                match api::put::<_, serde_json::Value>(
                    &format!("/api/books/{}/notes/{}", bid, nid),
                    &req,
                ).await {
                    Ok(_) => note_save_status.set("saved"),
                    Err(_) => note_save_status.set("error"),
                }
            });
        }
    };

    let schedule_note_save = move || {
        note_save_status.set("unsaved");
        let old_id = note_save_timer_id.get();
        if old_id != 0 {
            web_sys::window().unwrap().clear_timeout_with_handle(old_id);
        }
        let cb = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
            save_note_content();
        }) as Box<dyn FnMut()>);
        let id = web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                800,
            )
            .unwrap_or(0);
        cb.forget();
        note_save_timer_id.set(id);
    };

    let go_back_to_notes = move || {
        // Save before navigating back
        save_note_content();
        active_pane.set(BookPane::Notes);
        // Refresh notes list
        let bid = bid_signal.get();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(resp) = api::get::<NotesResponse>(&format!("/api/books/{}/notes", bid)).await {
                store.notes.set(resp.notes);
                store.note_tree.set(Some(resp.tree));
            }
        });
    };

    let go_back_to_chapters = move || {
        active_pane.set(BookPane::Chapters);
    };

    rsx! {
        Fragment {
            style { {TYPOGRAPHY_CSS} }
            style { {NOTES_CSS} }
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
                         .book-workspace .editor-topbar .rinch-text,
                         .book-workspace .editor-topbar .editor-title-input input {{ font-family: '{h1}', cursive; }}
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
                        span {
                            style: "flex: 1; overflow: hidden; text-overflow: ellipsis;",
                            {move || store.current_book.get().map(|b| b.title.clone()).unwrap_or_default()}
                        }
                        ActionIcon {
                            variant: "subtle",
                            size: "xs",
                            onclick: move || {
                                let book = store.current_book.get();
                                if let Some(b) = book {
                                    edit_book_title.set(b.title.clone());
                                    edit_book_desc.set(b.description.clone());
                                    show_book_settings_modal.set(true);
                                }
                            },
                            {render_tabler_icon(__scope, TablerIcon::Pencil, TablerIconStyle::Outline)}
                        }
                    }
                    Space { h: "sm" }

                    div { class: "book-sidebar-nav",
                        // Chapters section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Chapters) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            div {
                                style: "display: flex; align-items: center; cursor: pointer;",
                                onclick: move || chapters_collapsed.update(|c| *c = !*c),
                                span {
                                    style: "margin-right: 6px; display: inline-flex; align-items: center; font-size: 10px; line-height: 1; position: relative; top: -1px;",
                                    {move || if chapters_collapsed.get() { "\u{25b8}" } else { "\u{25be}" }}
                                }
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

                        // Notes section header
                        div {
                            class: {move || if matches!(active_pane.get(), BookPane::Notes | BookPane::NoteEditor(_)) { "sidebar-section-header active" } else { "sidebar-section-header" }},
                            div {
                                style: "display: flex; align-items: center; cursor: pointer; flex: 1;",
                                onclick: open_notes_pane,
                                "Notes"
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 2px;",
                                ActionIcon {
                                    variant: "subtle",
                                    size: "xs",
                                    onclick: move || {
                                        new_note_title.set(String::new());
                                        new_note_parent_id.set(None);
                                        new_note_color.set("teal".to_string());
                                        show_note_modal.set(true);
                                    },
                                    {render_tabler_icon(__scope, TablerIcon::Plus, TablerIconStyle::Outline)}
                                }
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
                                div {
                                    style: "display: flex; gap: 8px;",
                                    Button {
                                        size: "sm",
                                        variant: "outline",
                                        onclick: move || {
                                            show_import_modal.set(true);
                                            import_preview.set(Vec::new());
                                            import_full_chapters.set(Vec::new());
                                            import_error.set(None);
                                            import_filename.set(String::new());
                                        },
                                        "Import"
                                    }
                                    Button {
                                        size: "sm",
                                        onclick: move || {
                                            new_chapter_title.set(String::new());
                                            show_chapter_modal.set(true);
                                        },
                                        "Add Chapter"
                                    }
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
                                                    size: "sm",
                                                    onclick: {
                                                        let cid = chapter.id.clone();
                                                        let ctitle = chapter.title.clone();
                                                        move || {
                                                            rename_chapter_id.set(cid.clone());
                                                            rename_chapter_title.set(ctitle.clone());
                                                            show_rename_chapter_modal.set(true);
                                                        }
                                                    },
                                                    {render_tabler_icon(
                                                        __scope,
                                                        TablerIcon::Pencil,
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
                                TextInput {
                                    class: "editor-title-input",
                                    value_fn: move || chapter_title.get(),
                                    oninput: move |v: String| save_chapter_title_inline(v),
                                }
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
                                        new_beta_username.set(String::new());
                                        beta_link_error.set(None);
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
                                        edit_beta_username,
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

                    // Notes pane (CSS toggle)
                    div {
                        class: "notes-pane",
                        style: {move || if matches!(active_pane.get(), BookPane::Notes) { "" } else { "display:none;" }},

                        div { class: "notes-pane-header",
                            Title { order: 3, "Notes" }
                            Button {
                                size: "sm",
                                onclick: move || {
                                    new_note_title.set(String::new());
                                    new_note_parent_id.set(None);
                                    new_note_color.set("teal".to_string());
                                    show_note_modal.set(true);
                                },
                                "Add Note"
                            }
                        }

                        if store.notes.get().is_empty() {
                            div { class: "notes-empty",
                                Text { color: "dimmed", "No notes yet. Add one to start organizing your ideas!" }
                            }
                        }

                        if !store.notes.get().is_empty() {
                            div { class: "notes-tree",
                                for (idx, root_id) in store.note_tree.get().map(|t| t.root_order.clone()).unwrap_or_default().into_iter().enumerate() {
                                    div { style: "display: contents;",
                                        {render_drop_zone(__scope, Signal::new(None), idx, drop_target, dragging_note_id)}
                                        {render_note_card(
                                            __scope,
                                            root_id.clone(),
                                            store,
                                            active_pane,
                                            bid_signal,
                                            new_note_title,
                                            new_note_parent_id,
                                            new_note_color,
                                            show_note_modal,
                                            note_editor_title,
                                            note_editor_color,
                                            dragging_note_id,
                                            drop_target,
                                        )}
                                    }
                                }
                                {render_drop_zone(__scope, Signal::new(None), store.note_tree.get().map(|t| t.root_order.len()).unwrap_or(0), drop_target, dragging_note_id)}
                            }
                        }
                    }

                    // Note Editor pane (CSS toggle)
                    div {
                        style: {move || if matches!(active_pane.get(), BookPane::NoteEditor(_)) { "" } else { "display:none;" }},
                        class: "note-editor-pane",

                        div { class: "note-editor-topbar",
                            div { class: "note-editor-topbar-left",
                                ActionIcon {
                                    variant: "subtle",
                                    onclick: go_back_to_notes,
                                    {render_tabler_icon(__scope, TablerIcon::ArrowLeft, TablerIconStyle::Outline)}
                                }
                                Text { weight: "600", {move || note_editor_title.get()} }
                            }
                            div {
                                style: "display: flex; align-items: center; gap: 8px;",
                                div {
                                    class: "note-save-indicator",
                                    {move || match note_save_status.get() {
                                        "saving" => "Saving...".to_string(),
                                        "saved" => "Saved".to_string(),
                                        _ => "Unsaved".to_string(),
                                    }}
                                }
                            }
                        }

                        div { class: "note-editor-body",
                            TextInput {
                                label: "Title",
                                value_fn: move || note_editor_title.get(),
                                oninput: move |v: String| {
                                    note_editor_title.set(v);
                                    schedule_note_save();
                                },
                            }
                            Space { h: "sm" }
                            Text { size: "sm", weight: "500", "Color" }
                            Space { h: "xs" }
                            div { class: "note-color-picker",
                                for color in ["teal", "blue", "violet", "pink", "red", "orange", "yellow", "green", "gray"] {
                                    div {
                                        class: {
                                            let c = color.to_string();
                                            move || {
                                                let selected = note_editor_color.get().as_deref() == Some(&c) ||
                                                    (note_editor_color.get().is_none() && c == "teal");
                                                if selected { "note-color-dot selected" } else { "note-color-dot" }
                                            }
                                        },
                                        style: {
                                            let c = color.to_string();
                                            move || format!("background: var(--rinch-color-{}-6);", c)
                                        },
                                        onclick: {
                                            let c = color.to_string();
                                            move || {
                                                note_editor_color.set(Some(c.clone()));
                                                schedule_note_save();
                                            }
                                        },
                                    }
                                }
                            }
                            Space { h: "md" }
                            Text { size: "sm", weight: "500", "Content" }
                            Space { h: "xs" }

                            {editor_utils::editor_toolbar(__scope)}

                            div {
                                contenteditable: "true",
                                class: "editor-content",
                                id: "note-editor-main",
                                style: "min-height: 200px; padding: 12px; border: 1px solid var(--rinch-color-border); border-radius: var(--rinch-radius-sm);",
                                oninput: move |_: String| schedule_note_save(),
                            }
                        }
                    }
                }
            }

            // Add Note Modal
            Modal {
                opened_fn: move || show_note_modal.get(),
                onclose: move || show_note_modal.set(false),
                title: "Add Note",

                TextInput {
                    label: "Note Title",
                    placeholder: "Enter note title",
                    value_fn: move || new_note_title.get(),
                    oninput: move |v: String| new_note_title.set(v),
                    onsubmit: add_note,
                }
                Space { h: "sm" }
                Text { size: "sm", weight: "500", "Color" }
                Space { h: "xs" }
                div { class: "note-color-picker",
                    for color in ["teal", "blue", "violet", "pink", "red", "orange", "yellow", "green", "gray"] {
                        div {
                            class: {
                                let c = color.to_string();
                                move || if new_note_color.get() == c { "note-color-dot selected" } else { "note-color-dot" }
                            },
                            style: {
                                let c = color.to_string();
                                move || format!("background: var(--rinch-color-{}-6);", c)
                            },
                            onclick: {
                                let c = color.to_string();
                                move || new_note_color.set(c.clone())
                            },
                        }
                    }
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_note_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: add_note,
                        "Add"
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

            // Book Settings Modal
            Modal {
                opened_fn: move || show_book_settings_modal.get(),
                onclose: move || show_book_settings_modal.set(false),
                title: "Book Settings",

                TextInput {
                    label: "Title",
                    placeholder: "Book title",
                    value_fn: move || edit_book_title.get(),
                    oninput: move |v: String| edit_book_title.set(v),
                    onsubmit: save_book_settings,
                }
                Space { h: "md" }
                Textarea {
                    label: "Description",
                    placeholder: "What's this book about?",
                    value_fn: move || edit_book_desc.get(),
                    oninput: move |v: String| edit_book_desc.set(v),
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_book_settings_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: save_book_settings,
                        "Save"
                    }
                }
            }

            // Rename Chapter Modal
            Modal {
                opened_fn: move || show_rename_chapter_modal.get(),
                onclose: move || show_rename_chapter_modal.set(false),
                title: "Rename Chapter",

                TextInput {
                    label: "Chapter Title",
                    placeholder: "Enter chapter title",
                    value_fn: move || rename_chapter_title.get(),
                    oninput: move |v: String| rename_chapter_title.set(v),
                    onsubmit: save_rename_chapter,
                }
                Space { h: "lg" }
                Group {
                    justify: "flex-end",
                    Button {
                        variant: "subtle",
                        onclick: move || show_rename_chapter_modal.set(false),
                        "Cancel"
                    }
                    Button {
                        onclick: save_rename_chapter,
                        "Rename"
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
                Space { h: "md" }
                TextInput {
                    label: "PlotWeb Username (optional)",
                    placeholder: "Attach to a registered user",
                    value_fn: move || new_beta_username.get(),
                    oninput: move |v: String| new_beta_username.set(v),
                }
                Text { size: "xs", color: "dimmed",
                    "If set, this user will see the book on their dashboard."
                }
                if let Some(ref err) = beta_link_error.get() {
                    Space { h: "xs" }
                    Text { size: "xs", color: "red", {err.clone()} }
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
                Space { h: "md" }
                TextInput {
                    label: "PlotWeb Username (optional)",
                    placeholder: "Attach to a registered user",
                    value_fn: move || edit_beta_username.get(),
                    oninput: move |v: String| edit_beta_username.set(v),
                }
                Text { size: "xs", color: "dimmed",
                    "If set, this user will see the book on their dashboard. Clear to detach."
                }
                if let Some(ref err) = beta_link_error.get() {
                    Space { h: "xs" }
                    Text { size: "xs", color: "red", {err.clone()} }
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

            // Import Manuscript Modal
            Modal {
                opened_fn: move || show_import_modal.get(),
                onclose: move || {
                    show_import_modal.set(false);
                    import_preview.set(Vec::new());
                    import_full_chapters.set(Vec::new());
                    import_error.set(None);
                    import_filename.set(String::new());
                    import_file.set(None);
                },
                title: "Import Manuscript",

                if import_preview.get().is_empty() && !import_loading.get() {
                    // File picker step
                    Text { size: "sm", color: "dimmed",
                        "Upload a .md, .txt, or .docx file. Chapters will be detected automatically from headings, \"Chapter\" markers, or ALL-CAPS titles."
                    }
                    Space { h: "md" }
                    div {
                        style: "display: flex; flex-direction: column; align-items: center; padding: 32px; border: 2px dashed var(--rinch-color-border); border-radius: 8px; cursor: pointer;",
                        onclick: move || {
                            // Create a temporary file input, attach a change
                            // listener, and click it.  rinch's onchange passes
                            // a String (the input value) which is useless for
                            // file inputs, so we use the raw DOM API instead.
                            let doc = match web_sys::window().and_then(|w| w.document()) {
                                Some(d) => d,
                                None => return,
                            };
                            let input: web_sys::HtmlInputElement = doc
                                .create_element("input").unwrap()
                                .dyn_into().unwrap();
                            input.set_type("file");
                            input.set_accept(".md,.txt,.docx,.markdown");
                            input.style().set_property("display", "none").ok();
                            doc.body().unwrap().append_child(&input).ok();

                            let input_clone = input.clone();
                            let change_handler = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                                if let Some(files) = input_clone.files() {
                                    if let Some(file) = files.get(0) {
                                        import_loading.set(true);
                                        import_error.set(None);
                                        import_file.set(Some(file.clone()));
                                        let bid = bid_signal.get();
                                        let input_remove = input_clone.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            match api::upload_file::<ImportPreviewResponse>(
                                                &format!("/api/books/{}/import/preview", bid),
                                                &file,
                                            ).await {
                                                Ok(resp) => {
                                                    import_filename.set(resp.filename);
                                                    import_preview.set(resp.chapters);
                                                }
                                                Err(e) => {
                                                    import_error.set(Some(e.message));
                                                }
                                            }
                                            import_loading.set(false);
                                            input_remove.remove();
                                        });
                                    }
                                }
                            }) as Box<dyn FnMut(_)>);
                            input.add_event_listener_with_callback("change", change_handler.as_ref().unchecked_ref()).ok();
                            change_handler.forget();
                            input.click();
                        },
                        {render_tabler_icon(__scope, TablerIcon::Upload, TablerIconStyle::Outline)}
                        Space { h: "xs" }
                        Text { size: "sm", color: "dimmed", "Click to select file" }
                    }
                }

                if import_loading.get() {
                    Space { h: "lg" }
                    Center {
                        Text { color: "dimmed", "Analyzing manuscript..." }
                    }
                    Space { h: "lg" }
                }

                if let Some(err) = import_error.get() {
                    Space { h: "sm" }
                    Text { size: "sm", color: "red", {err} }
                }

                if !import_preview.get().is_empty() {
                    // Preview step
                    Text { size: "sm", color: "dimmed",
                        {move || format!("Found {} chapters in {}", import_preview.get().len(), import_filename.get())}
                    }
                    Space { h: "md" }
                    div {
                        style: "max-height: 400px; overflow-y: auto; display: flex; flex-direction: column; gap: 6px;",
                        for (i, ch) in import_preview.get().into_iter().enumerate() {
                            Paper {
                                key: format!("import-ch-{}", i),
                                p: "sm",
                                radius: "sm",
                                shadow: "xs",
                                div {
                                    style: "display: flex; align-items: center; gap: 8px; margin-bottom: 4px;",
                                    Badge { variant: "light", size: "sm", {format!("{}", i + 1)} }
                                    Text { weight: "600", size: "sm", {ch.title.clone()} }
                                    Text { size: "xs", color: "dimmed", {format!("{} words", ch.word_count)} }
                                }
                                Text { size: "xs", color: "dimmed",
                                    {ch.content_preview.clone()}
                                }
                            }
                        }
                    }
                    Space { h: "lg" }
                    Group {
                        justify: "flex-end",
                        Button {
                            variant: "subtle",
                            onclick: move || {
                                import_preview.set(Vec::new());
                                import_full_chapters.set(Vec::new());
                                import_error.set(None);
                                import_filename.set(String::new());
                            },
                            "Back"
                        }
                        Button {
                            onclick: move || {
                                let bid = bid_signal.get();
                                let file = match import_file.get() {
                                    Some(f) => f,
                                    None => return,
                                };
                                import_loading.set(true);
                                wasm_bindgen_futures::spawn_local(async move {
                                    match api::upload_file::<Vec<Chapter>>(
                                        &format!("/api/books/{}/import/confirm", bid),
                                        &file,
                                    ).await {
                                        Ok(new_chapters) => {
                                            store.chapters.update(|ch| ch.extend(new_chapters));
                                            show_import_modal.set(false);
                                            import_preview.set(Vec::new());
                                            import_full_chapters.set(Vec::new());
                                            import_error.set(None);
                                            import_filename.set(String::new());
                                            import_file.set(None);
                                        }
                                        Err(e) => {
                                            import_error.set(Some(e.message));
                                        }
                                    }
                                    import_loading.set(false);
                                });
                            },
                            {move || format!("Import {} Chapters", import_preview.get().len())}
                        }
                    }
                }
            }
        }
    }
}
