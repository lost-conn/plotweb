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

/// CSS for the notes surface — the outline, the view tabs and the filter bar.
///
/// `design/04-notes-wireframes.html` take E: a vertical outline rather than the
/// horizontal card tree this replaces. Facet is a one-character gutter glyph on the
/// left, the span sits in a right-aligned gutter, and the rest of the row is title.
/// Metadata (span, chips, counts) is set in a monospace face, which is the wireframe's
/// voice for anything the author did not write themselves.
pub(super) const NOTES_CSS: &str = r#"
.notes-pane {
    padding: 0;
    overflow-y: auto;
    overflow-x: hidden;
    height: 100%;
    display: flex;
    flex-direction: column;
}
.notes-pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--pw-space-lg) var(--pw-space-lg) 0 var(--pw-space-lg);
    flex-shrink: 0;
}

/* ── View switcher ──
   Tree and Timeline: two renderings of the same filtered, selected notes. */
.notes-views {
    display: flex;
    gap: var(--pw-space-md);
    padding: var(--pw-space-sm) var(--pw-space-lg) 0 var(--pw-space-lg);
    border-bottom: 1px solid var(--pw-hairline);
    flex-shrink: 0;
}
.notes-viewtab {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-dimmed);
    padding: 0 0 6px 0;
    margin-bottom: -1px;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    transition: color var(--pw-dur-fast) var(--pw-ease);
}
.notes-viewtab:hover {
    color: var(--rinch-color-text);
}
.notes-viewtab.is-on {
    color: var(--rinch-color-text);
    border-bottom-color: var(--rinch-color-teal-6);
}

/* ── Filter bar ──
   Shared by every notes view, which is why it is a sibling of the tree rather
   than part of it. A chip cycles off -> must -> any of -> without -> off. */
.notes-filter {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-2xs);
    padding: var(--pw-space-sm) var(--pw-space-lg) 0 var(--pw-space-lg);
    flex-shrink: 0;
}
.notes-filter-chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--pw-space-2xs);
}
.fchip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--pw-text-2xs);
    line-height: 1.4;
    padding: 3px 9px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
    background: var(--rinch-color-surface);
    color: var(--rinch-color-dimmed);
    cursor: pointer;
    user-select: none;
    transition: border-color var(--pw-dur-fast) var(--pw-ease),
                color var(--pw-dur-fast) var(--pw-ease);
}
.fchip:hover {
    border-color: var(--rinch-color-placeholder);
    color: var(--rinch-color-text);
}
.fchip .op {
    width: 8px;
    text-align: center;
    font-weight: 700;
}
.fchip[data-state="must"] {
    border-color: var(--rinch-color-teal-6);
    color: var(--rinch-color-teal-6);
}
/* Dashed for the OR group: the members belong together and none of them is
   individually required, which a solid border would imply. */
.fchip[data-state="any"] {
    border-style: dashed;
    border-color: var(--rinch-color-teal-6);
    color: var(--rinch-color-teal-6);
}
.fchip[data-state="without"] {
    border-color: var(--rinch-color-red-6);
    color: var(--rinch-color-red-6);
    text-decoration: line-through;
}
.notes-filter-clear {
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-dimmed);
    padding: 3px 4px;
    cursor: pointer;
}
.notes-filter-clear:hover {
    color: var(--rinch-color-text);
}
.notes-filter-status {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-dimmed);
}

/* ── The outline ── */
.notes-tree {
    display: flex;
    flex-direction: column;
    padding: var(--pw-space-xs) var(--pw-space-lg) var(--pw-space-2xl) var(--pw-space-lg);
}
.notes-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--pw-space-2xl) var(--pw-space-lg);
}
.note-branch {
    display: flex;
    flex-direction: column;
    min-width: 0;
}
.note-row {
    display: flex;
    align-items: center;
    gap: var(--pw-space-3xs);
    border-radius: var(--pw-radius-sm);
    min-width: 0;
}
.note-row:hover {
    background: var(--pw-hairline);
}
.note-row.is-selected {
    background: var(--pw-hairline);
}
.note-row.is-selected .note-card-title {
    font-weight: 600;
}
/* Shown only because something below it matched: a path to a result, not a
   result. Dimmed rather than dropped, or a match filed three levels down would
   take its ancestors off the screen with it. */
.note-row.is-muted {
    opacity: 0.45;
}
.note-twist {
    width: 16px;
    flex: none;
    align-self: stretch;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 9px;
    line-height: 1;
    color: var(--rinch-color-placeholder);
    cursor: pointer;
}
.note-card {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
    padding: 6px var(--pw-space-2xs);
    cursor: pointer;
    border-radius: var(--pw-radius-sm);
}
.note-glyph {
    width: 14px;
    flex: none;
    text-align: center;
    font-size: 11px;
    line-height: 1;
    color: var(--note-color, var(--rinch-color-teal-6));
}
.note-glyph.is-lore {
    color: var(--rinch-color-placeholder);
}
.note-card-title {
    flex: 1;
    min-width: 0;
    font-size: var(--pw-text-md);
    color: var(--rinch-color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.note-when {
    flex: none;
    margin-left: auto;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--pw-text-2xs);
    font-variant-numeric: tabular-nums;
    color: var(--rinch-color-dimmed);
    /* An invented calendar writes longer dates than a bare year ("yr 1206, dry,
       day 12, bell 9"), so the gutter gives way before the title does. The full
       reading stays in the row's tooltip and the note's facet strip. */
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.note-row-actions {
    display: flex;
    gap: 2px;
    flex: none;
    opacity: 0;
    transition: opacity var(--pw-dur-fast) var(--pw-ease);
}
.note-row:hover .note-row-actions {
    opacity: 1;
}
.note-children {
    display: flex;
    flex-direction: column;
    margin-left: 15px;
    padding-left: var(--pw-space-xs);
    border-left: 1px solid var(--pw-hairline);
    min-width: 0;
}
/* The gap opens instantly. It used to grow over 100ms, which on the old 48px+
   cards was decoration and on these ~35px rows is a moving target: every row
   below the cursor slides down while you are aiming at one, and a third of a
   row is all the difference between "insert before" and "nest inside". */
.note-drop-zone {
    height: 0;
    border-radius: 2px;
    transition: background 0.1s;
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
    box-shadow: inset 0 0 0 1px var(--rinch-color-teal-6);
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
/* "Notes here" — the `@` edges pointing at the open chapter, read from the
   manuscript side. Fades with the rest of the chrome while typing, so it is
   reference material between sessions rather than something beside the prose. */
.chapter-backlinks {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    padding: 4px 20px 8px;
    flex-shrink: 0;
    transition: opacity var(--pw-dur-slow, 320ms) var(--pw-ease, ease);
}
.editor-layout.is-writing .chapter-backlinks {
    opacity: 0;
    pointer-events: none;
}
.chapter-backlinks-label {
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rinch-color-dimmed);
}
.chapter-backlink {
    background: transparent;
    border: 1px solid var(--rinch-color-border);
    border-radius: 999px;
    padding: 1px 9px;
    font: inherit;
    font-size: 12px;
    color: var(--rinch-color-text);
    cursor: pointer;
}
.chapter-backlink:hover {
    border-color: var(--rinch-color-teal-6);
}

.note-editor-split {
    flex: 1;
    display: flex;
    align-items: stretch;
    min-height: 0;
}
.note-editor-body {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    min-width: 0;
}

/* ── Facet strip ──────────────────────────────────────────────────────────
   Event and entity are not exclusive, so they read as two independent marks
   rather than a segmented control, which would imply picking one. */
.note-facets {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 16px;
    flex-wrap: wrap;
}
.note-facet {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    border-radius: 999px;
    border: 1px solid var(--rinch-color-border);
    font-size: 12px;
    color: var(--rinch-color-dimmed);
    background: transparent;
    user-select: none;
}
.note-facet.is-button {
    cursor: pointer;
}
.note-facet.is-button:hover {
    border-color: var(--rinch-color-teal-6);
}
.note-facet.is-on {
    color: var(--rinch-color-text);
    border-color: var(--rinch-color-teal-6);
    background: color-mix(in srgb, var(--rinch-color-teal-6) 12%, transparent);
}
.note-facet-glyph {
    font-size: 11px;
    line-height: 1;
}
.note-facet-span {
    font-size: 12px;
    color: var(--rinch-color-dimmed);
    margin-left: auto;
}

/* ── Context rail ─────────────────────────────────────────────────────────
   Beside the prose, not over it: the rail is reference material, and a panel
   that covered what was being written would read as a dialog. */
.note-rail {
    width: 240px;
    flex-shrink: 0;
    overflow-y: auto;
    padding: 24px 16px;
    border-left: 1px solid var(--rinch-color-border);
    font-size: 13px;
}
.note-rail-group {
    margin-bottom: 20px;
}
.note-rail-heading {
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rinch-color-dimmed);
    margin-bottom: 6px;
}
.note-rail-empty {
    color: var(--rinch-color-dimmed);
    font-size: 12px;
    font-style: italic;
}
.note-rail-row {
    display: flex;
    align-items: baseline;
    gap: 6px;
    padding: 3px 6px;
    margin: 0 -6px;
    border-radius: var(--rinch-radius-sm);
}
.note-rail-row.is-openable {
    cursor: pointer;
}
.note-rail-row.is-openable:hover {
    background: var(--rinch-color-surface-hover, rgba(127, 127, 127, 0.12));
}
.note-rail-glyph {
    color: var(--rinch-color-dimmed);
    font-size: 11px;
    flex-shrink: 0;
}
.note-rail-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.note-rail-missing {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
    font-style: italic;
    margin-left: auto;
    flex-shrink: 0;
}
.note-rail-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
}
.note-rail-tag {
    padding: 1px 7px;
    border-radius: 999px;
    border: 1px solid var(--rinch-color-border);
    font-size: 11px;
    color: var(--rinch-color-dimmed);
}

/* ── Sigil completion menu ────────────────────────────────────────────────
   Fixed, so it is placed from the caret's viewport coordinates without caring
   which scroll container the editor sits in. */
.sigil-menu {
    position: fixed;
    left: 0;
    top: 0;
    z-index: 9000;
    min-width: 200px;
    max-width: 320px;
    padding: 4px;
    border-radius: var(--rinch-radius-sm);
    border: 1px solid var(--rinch-color-border);
    background: var(--rinch-color-body, var(--rinch-color-surface));
    box-shadow: var(--pw-shadow-3, 0 8px 24px rgba(0, 0, 0, 0.18));
}
.sigil-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 5px 8px;
    border-radius: var(--rinch-radius-sm);
    cursor: pointer;
    /* rinch fires onclick on pointerdown, so a finger that drifts a pixel must
       not start a text selection instead of choosing the row. */
    user-select: none;
    touch-action: manipulation;
}
.sigil-row:hover,
.sigil-row.selected {
    background: color-mix(in srgb, var(--rinch-color-teal-6) 16%, transparent);
}
.sigil-row-glyph {
    color: var(--rinch-color-dimmed);
    font-size: 11px;
    width: 12px;
    flex-shrink: 0;
}
.sigil-row-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.sigil-row-kind {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
    flex-shrink: 0;
}

/* Phone: the rail stacks under the prose rather than squeezing it to nothing,
   and the menu takes the width it needs to stay readable. */
@media (max-width: 900px) {
    .note-editor-split {
        flex-direction: column;
    }
    .note-rail {
        width: auto;
        border-left: none;
        border-top: 1px solid var(--rinch-color-border);
        padding: 16px 24px;
    }
    .sigil-menu {
        max-width: calc(100vw - 32px);
    }
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

/* ── Phone ──
   The outline is already vertical, so nothing has to change shape: only the
   page gutter shrinks, and the row actions stop being hover-only. */
@media (max-width: 768px) {
    .notes-pane-header,
    .notes-views,
    .notes-filter {
        padding-left: var(--pw-space-md);
        padding-right: var(--pw-space-md);
    }
    .notes-tree {
        padding-left: var(--pw-space-md);
        padding-right: var(--pw-space-md);
    }
    .note-children {
        margin-left: 10px;
    }
    .note-row-actions {
        opacity: 1;
    }
}
/* Coarse pointers (touch) can't hover — always reveal the row actions. */
@media (hover: none) {
    .note-row-actions {
        opacity: 1;
    }
}

/* ── Time and the calendar (notes card 4) ─────────────────────────────────
   The time field opens under the facet strip, in the flow of the note rather
   than over it; the calendar is a pane of its own, one card per unit. */
.notes-pane-header-actions {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
}
.note-facets-block {
    margin-bottom: 16px;
}
.note-facets-block .note-facets {
    margin-bottom: 0;
}
.note-facet-span {
    cursor: pointer;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-variant-numeric: tabular-nums;
    border-bottom: 1px dashed transparent;
}
.note-facet-span:hover {
    color: var(--rinch-color-text);
    border-bottom-color: var(--rinch-color-teal-6);
}
.note-time-editor {
    margin-top: 10px;
    padding: 10px 12px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    background: color-mix(in srgb, var(--rinch-color-teal-6) 5%, transparent);
}
.note-time-row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
}
.note-time-input {
    flex: 1 1 240px;
    min-width: 0;
}
.note-time-input input {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.note-time-preview {
    margin-top: 6px;
    font-size: 12px;
    color: var(--rinch-color-teal-7);
}
.note-time-preview.is-error {
    color: var(--rinch-color-red-7);
}
.note-time-help {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 4px;
    font-size: 11px;
    color: var(--rinch-color-dimmed);
}
.note-time-calendar {
    cursor: pointer;
    color: var(--rinch-color-teal-7);
}
.note-time-calendar:hover {
    text-decoration: underline;
}

.calendar-pane {
    padding-bottom: var(--pw-space-xl);
}
.calendar-head {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
}
.calendar-lede,
.calendar-note {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-dimmed);
    max-width: 60ch;
    margin: var(--pw-space-xs) 0 var(--pw-space-md) 0;
}
.cal-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
}
.cal-field-label {
    font-size: 11px;
    color: var(--rinch-color-dimmed);
    letter-spacing: 0.02em;
}
.cal-field input {
    font: inherit;
    font-size: 13px;
    padding: 5px 8px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--rinch-radius-sm);
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    min-width: 0;
    width: 100%;
    box-sizing: border-box;
}
.cal-field input:focus {
    outline: none;
    border-color: var(--rinch-color-teal-6);
}
.calendar-name {
    max-width: 320px;
    margin-bottom: var(--pw-space-md);
}
.cal-rows {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-sm);
}
.cal-row {
    border: 1px solid var(--pw-hairline);
    border-radius: var(--rinch-radius-sm);
    padding: 10px 12px 12px;
}
.cal-row-head {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--rinch-color-dimmed);
    margin-bottom: 6px;
}
.cal-row-fields {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px 12px;
}
.cal-row-actions,
.calendar-actions {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
    flex-wrap: wrap;
    margin-top: var(--pw-space-sm);
}
.calendar-preview {
    margin-top: var(--pw-space-md);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    color: var(--rinch-color-text);
}
.calendar-preview.is-error {
    font-family: inherit;
    color: var(--rinch-color-red-7);
}
.span-rule {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-xs);
    margin-top: var(--pw-space-lg);
    padding-top: var(--pw-space-md);
    border-top: 1px solid var(--pw-hairline);
}
.span-rule-options {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--pw-space-xs);
}
.span-rule-option {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--pw-space-sm);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
    background: var(--rinch-color-surface);
    cursor: pointer;
    user-select: none;
}
.span-rule-option:hover {
    border-color: var(--rinch-color-placeholder);
}
.span-rule-option.is-on {
    border-color: var(--rinch-color-teal-6);
    background: color-mix(in srgb, var(--rinch-color-teal-6) 10%, var(--rinch-color-surface));
}
.span-rule-name {
    font-size: var(--pw-text-sm);
    font-weight: 600;
    color: var(--rinch-color-text);
}
.span-rule-says {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
}
.calendar-status,
.span-rule-status {
    font-size: 12px;
    color: var(--rinch-color-dimmed);
}
@media (max-width: 768px) {
    .cal-row-fields {
        grid-template-columns: repeat(2, minmax(0, 1fr));
    }
}
@media (max-width: 420px) {
    .cal-row-fields {
        grid-template-columns: minmax(0, 1fr);
    }
    .note-when {
        max-width: 38%;
    }
}

/* ── The timeline: entity lanes (card 5) ──
   take C of design/04-notes-wireframes.html, as the lanes zone of the stacked
   ribbon-over-lanes view. The drawing is SVG with a minimum width inside its own
   horizontal scroller, so a phone scrolls the drawing, never the page. */
.tl {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-xs);
    padding: var(--pw-space-sm) var(--pw-space-lg) var(--pw-space-2xl) var(--pw-space-lg);
    min-width: 0;
}
.tl-bar {
    display: flex;
    align-items: flex-start;
    gap: var(--pw-space-xs);
    flex-wrap: wrap;
}
.tl-bar-lbl,
.tl-status,
.tl-order,
.tl-rail-title,
.tl-held-why,
.tl-drop-label {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--pw-text-2xs);
}
.tl-bar-lbl {
    color: var(--rinch-color-dimmed);
    padding-top: 4px;
}
.tl-follow {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    flex: 1 1 220px;
    min-width: 0;
}
.tl-chip {
    font-size: var(--pw-text-xs);
    line-height: 1.4;
    padding: 2px 10px;
    border: 1px solid var(--rinch-color-border);
    border-radius: 999px;
    background: var(--rinch-color-surface);
    color: var(--rinch-color-text);
    cursor: pointer;
    user-select: none;
}
.tl-chip:hover {
    border-color: var(--rinch-color-placeholder);
}
.tl-chip.is-on {
    background: var(--rinch-color-teal-6);
    border-color: var(--rinch-color-teal-6);
    color: #fff;
}
.tl-order {
    color: var(--rinch-color-dimmed);
    padding: 3px 9px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
    cursor: pointer;
    user-select: none;
    white-space: nowrap;
}
.tl-order:hover {
    color: var(--rinch-color-text);
}
.tl-status {
    color: var(--rinch-color-dimmed);
}
.tl-hint {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-dimmed);
}
.tl-scroll {
    overflow-x: auto;
    overflow-y: hidden;
    border: 1px solid var(--pw-hairline);
    border-radius: var(--pw-radius-sm);
    background: var(--rinch-color-surface);
}
.tl-canvas {
    position: relative;
    min-width: 760px;
}
.tl-svg {
    display: block;
    width: 100%;
    height: auto;
}
.tl-svg text {
    font-size: 11px;
    fill: var(--rinch-color-text);
}
.tl-tick line {
    stroke: var(--pw-hairline);
    stroke-width: 1;
}
.tl-tick.is-major line {
    stroke: var(--rinch-color-border);
}
.tl-svg .tl-tick text {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    fill: var(--rinch-color-dimmed);
}
.tl-axis {
    stroke: var(--rinch-color-border);
    stroke-width: 1;
}
.tl-band {
    fill: var(--pw-hairline);
    stroke: var(--rinch-color-border);
    stroke-width: 1;
}
/* Approximate: placed, not measured. */
.tl-band.is-fuzzy {
    stroke-dasharray: 3 3;
}
.tl-band.is-on {
    fill: color-mix(in srgb, var(--rinch-color-teal-6) 14%, transparent);
    stroke: var(--rinch-color-teal-6);
}
.tl-leader {
    stroke: var(--rinch-color-border);
    stroke-width: 1;
}
.tl-svg .tl-caption text {
    font-size: 10px;
    fill: var(--rinch-color-dimmed);
}
.tl-svg .tl-caption.is-on text {
    fill: var(--rinch-color-teal-6);
    font-weight: 600;
}
.tl-caption.is-on .tl-leader {
    stroke: var(--rinch-color-teal-6);
}
.tl-lane line {
    stroke: var(--rinch-color-border);
    stroke-width: 1;
}
.tl-lane.is-on line {
    stroke: var(--rinch-color-teal-6);
    stroke-width: 2;
}
/* Here only because a drawn event names this entity — the tree's "path to a
   result, not a result". */
.tl-lane.is-muted {
    opacity: 0.5;
}
.tl-svg .tl-lane.is-on .tl-lane-name text {
    fill: var(--rinch-color-teal-6);
    font-weight: 600;
}
/* The drawing takes no clicks; the HTML boxes over it do (see `Hit` in
   panes/timeline.rs for why). */
.tl-svg {
    pointer-events: none;
}
.tl-hits {
    position: absolute;
    inset: 0;
}
.tl-hit {
    position: absolute;
    cursor: pointer;
    border-radius: 2px;
}
.tl-hit-lane:hover,
.tl-hit-caption:hover,
.tl-hit-mark:hover {
    background: color-mix(in srgb, var(--rinch-color-teal-6) 8%, transparent);
}
.tl-life {
    stroke: var(--rinch-color-dimmed);
    stroke-width: 5;
    stroke-linecap: round;
    opacity: 0.3;
}
.tl-life.is-fuzzy {
    stroke-dasharray: 6 4;
}
.tl-life.is-on {
    stroke: var(--rinch-color-teal-6);
    opacity: 0.45;
}
.tl-tie {
    stroke: var(--rinch-color-dimmed);
    stroke-width: 1.5;
}
.tl-mark.is-on .tl-tie {
    stroke: var(--rinch-color-teal-6);
}
.tl-dot {
    fill: var(--rinch-color-text);
}
.tl-dot.is-on {
    fill: var(--rinch-color-teal-6);
}
/* ── The event ribbon (card 6) ── */
.tl-zones {
    display: flex;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
    overflow: hidden;
}
.tl-zone {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: var(--pw-text-2xs);
    padding: 3px 9px;
    color: var(--rinch-color-dimmed);
    cursor: pointer;
    user-select: none;
}
.tl-zone + .tl-zone {
    border-left: 1px solid var(--rinch-color-border);
}
.tl-zone.is-on {
    background: color-mix(in srgb, var(--rinch-color-teal-6) 14%, transparent);
    color: var(--rinch-color-text);
}
.tl-refusal {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-red-6);
}
.tl-svg .tl-zone-name {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 9px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    fill: var(--rinch-color-placeholder);
}
.tl-zone-rule line {
    stroke: var(--rinch-color-border);
    stroke-dasharray: 2 3;
}
/* A parent's containment band. Nested bands layer, so depth reads as tone. */
.tl-contain {
    fill: color-mix(in srgb, var(--rinch-color-teal-6) 7%, transparent);
    stroke: color-mix(in srgb, var(--rinch-color-teal-6) 25%, transparent);
    stroke-width: 1;
}
.tl-event-bar {
    fill: var(--rinch-color-surface);
    stroke: var(--rinch-color-border);
    stroke-width: 1;
}
.tl-event.depth-1 .tl-event-bar,
.tl-event.depth-2 .tl-event-bar,
.tl-event.depth-3 .tl-event-bar {
    fill: color-mix(in srgb, var(--rinch-color-text) 5%, var(--rinch-color-surface));
}
.tl-event.is-fuzzy .tl-event-bar {
    stroke-dasharray: 3 3;
}
/* No dates of its own: an outline drawn from what it contains. */
.tl-event.is-derived .tl-event-bar {
    fill: transparent;
    stroke-dasharray: 1 3;
    stroke: var(--rinch-color-dimmed);
}
.tl-event.is-pinned .tl-event-bar {
    stroke-width: 2;
}
.tl-event.is-muted {
    opacity: 0.5;
}
.tl-event.is-on .tl-event-bar {
    stroke: var(--rinch-color-teal-6);
    stroke-width: 2;
}
.tl-event.is-escaping .tl-event-bar {
    stroke: var(--rinch-color-orange-6);
}
.tl-clip {
    fill: none;
    stroke: var(--rinch-color-orange-6);
    stroke-width: 1.5;
}
.tl-escape circle {
    fill: var(--rinch-color-orange-6);
}
.tl-svg .tl-escape text {
    font-size: 9px;
    font-weight: 700;
    fill: #fff;
}
.tl-svg .tl-event-label {
    font-size: 11px;
    fill: var(--rinch-color-text);
}
.tl-svg .tl-event-label.is-beside {
    fill: var(--rinch-color-dimmed);
}
.tl-svg .tl-event-label.is-muted {
    opacity: 0.6;
}
.tl-svg .tl-event-label.is-on {
    fill: var(--rinch-color-teal-6);
    font-weight: 600;
}
.tl-hit-bar:hover,
.tl-hit-label:hover {
    background: color-mix(in srgb, var(--rinch-color-teal-6) 10%, transparent);
}
.tl-hit-bar.is-drop-into {
    outline: 2px solid var(--rinch-color-teal-6);
    background: color-mix(in srgb, var(--rinch-color-teal-6) 16%, transparent);
}
.tl-hit-bar.is-drop-refused {
    outline: 2px dashed var(--rinch-color-red-6);
    cursor: not-allowed;
}
/* The empty ribbon, as a drop target: armed only while a bar is dragged. */
.tl-ribbon-drop {
    position: absolute;
    left: 0;
    right: 0;
    pointer-events: none;
}
.tl-ribbon-drop.is-armed {
    pointer-events: auto;
    outline: 1px dashed var(--rinch-color-border);
}
.tl-ribbon-drop.is-drop-out {
    outline-color: var(--rinch-color-teal-6);
    background: color-mix(in srgb, var(--rinch-color-teal-6) 5%, transparent);
}
/* A bar's drag handle, just left of the bar. Owns the finger from first contact, like
   a held chip (see `.tl-held`), so a long-press can become a drag. */
.tl-handle {
    position: absolute;
    width: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    line-height: 1;
    color: var(--rinch-color-dimmed);
    opacity: 0.45;
    cursor: grab;
    user-select: none;
    touch-action: none;
    border-radius: 2px;
}
.tl-handle:hover,
.tl-handle.is-dragging {
    opacity: 1;
    background: color-mix(in srgb, var(--rinch-color-teal-6) 14%, transparent);
}
/* The drop target. Inert until a held note is being dragged, so it never sits
   over the drawing's own clicks. */
.tl-drop {
    position: absolute;
    top: 0;
    bottom: 0;
    pointer-events: none;
}
.tl-drop.is-armed {
    pointer-events: auto;
    background: color-mix(in srgb, var(--rinch-color-teal-6) 5%, transparent);
    outline: 1px dashed var(--rinch-color-teal-6);
}
.tl-drop-guide {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 0;
    border-left: 2px solid var(--rinch-color-teal-6);
    pointer-events: none;
}
.tl-drop-label {
    position: absolute;
    top: 2px;
    left: 4px;
    white-space: nowrap;
    color: var(--rinch-color-teal-6);
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-teal-6);
    border-radius: var(--pw-radius-sm);
    padding: 1px 5px;
}
.tl-rail {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-xs);
    padding-top: var(--pw-space-sm);
    border-top: 1px dashed var(--rinch-color-border);
}
.tl-rail-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
}
.tl-rail-title {
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--rinch-color-dimmed);
}
.tl-rail-sub,
.tl-rail-empty {
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-placeholder);
}
.tl-rail-items {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
}
.tl-held {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    padding: 4px 10px;
    border: 1px dashed var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
    background: var(--rinch-color-surface);
    cursor: grab;
    user-select: none;
    /* A long-press drag has to own the finger from the first contact: a browser
       decides whether a touch may pan when it starts, so a chip that allowed
       panning would have its drag cancelled (pointercancel) by the first move
       after the hold. The chips are small, so the page still scrolls from
       anywhere else. */
    touch-action: none;
}
.tl-held:hover {
    border-color: var(--rinch-color-teal-6);
}
.tl-held-glyph {
    color: var(--rinch-color-teal-6);
    font-size: 11px;
}
.tl-held-title {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.tl-held-why {
    color: var(--rinch-color-dimmed);
    white-space: nowrap;
}
@media (max-width: 768px) {
    .tl {
        padding-left: var(--pw-space-md);
        padding-right: var(--pw-space-md);
    }
}
"#;

/// CSS for the book workspace layout.
///
/// Stage 4b (`design/02-screens.html` `#chapters`): content up, tools down.
/// The sidebar's `.ws-*` classes are the mockup's own names; `.sidebar-*`
/// class names on the DOM nodes themselves predate this pass and are kept so
/// this diff doesn't also have to touch every call site that toggles
/// `.active`/`.open`.
pub(super) const BOOK_WORKSPACE_CSS: &str = r#"
.book-workspace {
    display: flex;
    height: 100dvh;
}

.book-sidebar {
    width: 250px;
    min-width: 250px;
    background: var(--pw-color-deep);
    border-right: 1px solid var(--rinch-color-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

/* ── Book header: mini jacket + title + word count ──── */

.ws-book {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: var(--pw-space-sm);
    border-bottom: 1px solid var(--pw-hairline);
    cursor: pointer;
    flex-shrink: 0;
}

/* Same trap as the dashboard jacket: the gradient ends at --pw-color-deepest,
   which is darker than the sidebar it sits on, and a 0.4-alpha black shadow is
   invisible on #1A1714. Without the hairline this reads as a stray vertical
   line rather than a book. */
.ws-book-mini {
    width: 26px;
    height: 36px;
    min-width: 26px;
    border-radius: 1px var(--pw-radius-sm) var(--pw-radius-sm) 1px;
    background: linear-gradient(150deg, var(--rinch-color-surface), var(--pw-color-deepest));
    border: 1px solid var(--rinch-color-border);
    border-left: 3px solid var(--rinch-color-teal-7);
    box-shadow: var(--pw-shadow-1);
}

.ws-book-title {
    font-family: var(--pw-font-display);
    font-size: var(--pw-text-md);
    line-height: 1.15;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.ws-book-words {
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-placeholder);
    margin-top: 2px;
}

.book-sidebar-nav {
    flex: 1;
    overflow-y: auto;
    padding: var(--pw-space-2xs) var(--pw-space-2xs) var(--pw-space-xs);
}

.sidebar-chapter-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: var(--pw-space-3xs) 0 var(--pw-space-2xs);
}

.sidebar-chapter-item {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
    padding: 6px var(--pw-space-xs);
    border-radius: var(--pw-radius-sm);
    cursor: pointer;
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-dimmed);
    position: relative;
    transition: background var(--pw-dur-fast) var(--pw-ease), color var(--pw-dur-fast) var(--pw-ease);
}

.sidebar-chapter-item:hover {
    background: var(--pw-hairline);
    color: var(--rinch-color-text);
}

.sidebar-chapter-item.active {
    background: var(--pw-hairline);
    color: var(--rinch-color-text);
}

.sidebar-chapter-item.active::before {
    content: '';
    position: absolute;
    left: -6px;
    top: 6px;
    bottom: 6px;
    width: 2px;
    background: var(--rinch-color-teal-6);
    border-radius: 0 2px 2px 0;
}

.sidebar-chapter-name {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
}

.sidebar-chapter-wc {
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-placeholder);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
}

/* ── Tools footer strip ──────────────────────────────── */

.ws-tools {
    border-top: 1px solid var(--pw-hairline);
    padding: var(--pw-space-xs) var(--pw-space-sm);
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
}

.ws-tools .sp {
    flex: 1;
}

.tool {
    position: relative;
    padding: 6px;
    border-radius: var(--pw-radius-sm);
    color: var(--rinch-color-dimmed);
    cursor: pointer;
    display: flex;
    transition: background var(--pw-dur-fast) var(--pw-ease), color var(--pw-dur-fast) var(--pw-ease);
}

.tool:hover {
    background: var(--pw-hairline);
    color: var(--rinch-color-text);
}

.tool[data-tip]:hover::after {
    content: attr(data-tip);
    position: absolute;
    bottom: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
    box-shadow: var(--pw-shadow-2);
    padding: 3px 7px;
    font-size: var(--pw-text-2xs);
    white-space: nowrap;
    color: var(--rinch-color-text);
    z-index: var(--pw-z-popover);
}

.tool .badge {
    position: absolute;
    top: 1px;
    right: 1px;
    min-width: 13px;
    height: 13px;
    border-radius: 999px;
    background: var(--rinch-color-teal-7);
    color: #fff;
    font-size: 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 3px;
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
    padding: var(--pw-space-xl) var(--pw-space-2xl);
}

/* ── Chrome collapse while typing (editor pane, Stage 5) ──────────────
   `.book-workspace.is-writing` is set for as long as the author is actively
   typing in the chapter editor (see `editor_writing` in book/mod.rs). Fades
   the sidebar with opacity + pointer-events, never width/margin/display —
   the prose column's x position must not move when this triggers, since the
   cursor is mid-line when it does. The editor's own header/footer/rail fade
   the same way; see EDITOR_CSS for those rules. */
.book-sidebar {
    transition: opacity var(--pw-dur-slow) var(--pw-ease);
}
.book-workspace.is-writing .book-sidebar {
    opacity: 0;
    pointer-events: none;
}

/* ── Chapters pane: hairline rows, not cards ─────────── */

.chapters-pane {
    max-width: var(--pw-pane-max);
    margin: 0 auto;
}

.chapters-menu {
    display: flex;
    flex-direction: column;
    min-width: 140px;
}

.chapters-menu-item {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
    font: inherit;
    font-size: var(--pw-text-sm);
    background: transparent;
    border: none;
    border-radius: var(--pw-radius-sm);
    padding: 7px var(--pw-space-sm);
    color: var(--rinch-color-text);
    cursor: pointer;
    text-align: left;
    transition: background var(--pw-dur-fast) var(--pw-ease);
}

.chapters-menu-item:hover {
    background: var(--pw-hairline);
}

.chapter-rows {
    display: flex;
    flex-direction: column;
}

.crow {
    display: flex;
    align-items: center;
    gap: var(--pw-space-sm);
    padding: 11px var(--pw-space-xs);
    border-bottom: 1px solid var(--pw-hairline);
    cursor: pointer;
    position: relative;
}

.crow:hover {
    background: var(--pw-hairline);
}

.crow.dragging {
    opacity: 0.4;
}

.crow.drop-target {
    box-shadow: inset 0 2px 0 var(--rinch-color-teal-6);
}

.crow .grip {
    color: var(--rinch-color-placeholder);
    cursor: grab;
    opacity: 0;
    display: flex;
    transition: opacity var(--pw-dur-fast) var(--pw-ease);
}

.crow:hover .grip {
    opacity: 1;
}

.crow .n {
    width: 22px;
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-placeholder);
    font-variant-numeric: tabular-nums;
    text-align: right;
    flex-shrink: 0;
}

.crow .t {
    flex: 1;
    font-size: var(--pw-text-md);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.crow .m {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
}

.crow .acts {
    display: flex;
    gap: 2px;
    opacity: 0;
    flex-shrink: 0;
    transition: opacity var(--pw-dur-fast) var(--pw-ease);
}

.crow:hover .acts {
    opacity: 1;
}

/* Tier 1 (design/01-language.html#overlays): the chapter row's title cell
   in its inline-edit state — rename, or the draft row from "Add chapter".
   Same box as the static `.crow .t` span so the row doesn't reflow when it
   switches between the two. */
.crow-edit-input {
    font: inherit;
    color: inherit;
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-teal-7);
    border-radius: var(--pw-radius-sm);
    padding: 1px 6px;
    box-shadow: var(--pw-focus-ring);
}

.crow-edit-input:focus {
    outline: none;
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
