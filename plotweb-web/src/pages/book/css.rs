//! CSS for the book workspace: typography settings, notes tree, and overall layout.
//! Extracted verbatim from the pre-split `book.rs` — no visual changes.

/// CSS for the typography settings section.
pub(super) const TYPOGRAPHY_CSS: &str = r#"
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
pub(super) const NOTES_CSS: &str = r#"
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
.note-drag-ghost {
    display: none;
    position: fixed;
    left: 0;
    top: 0;
    transform: translate(-50%, -50%) rotate(-2deg);
    z-index: 9999;
    pointer-events: none;
    max-width: 220px;
    padding: 8px 10px;
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-left: 3px solid var(--note-color, var(--rinch-color-teal-6));
    box-shadow: 0 6px 18px rgba(0,0,0,0.3);
    opacity: 0.9;
    font-size: 13px;
    font-weight: 600;
    color: var(--rinch-color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

/* ── Mobile: vertical stack, full-width cards, indented nesting ─────────── */
@media (max-width: 768px) {
    .notes-pane {
        overflow-x: hidden;
    }
    .notes-tree {
        min-width: 0;
    }
    .note-branch {
        flex-direction: column;
        align-items: stretch;
        width: 100%;
    }
    .note-card {
        width: auto;
    }
    .note-children {
        padding-left: 14px;
    }
    /* The horizontal connector stub doesn't apply to the vertical layout. */
    .note-child-row::before {
        display: none;
    }
    /* Actions are hover-only on desktop; always show them on touch. */
    .note-card-actions {
        opacity: 1;
    }
}
/* Coarse pointers (touch) can't hover — always reveal the card actions. */
@media (hover: none) {
    .note-card-actions {
        opacity: 1;
    }
}
"#;

/// CSS for the book workspace layout.
pub(super) const BOOK_WORKSPACE_CSS: &str = r#"
.book-workspace {
    display: flex;
    height: 100dvh;
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
