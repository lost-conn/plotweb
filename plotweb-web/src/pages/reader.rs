use wasm_bindgen::JsCast;
use rinch::prelude::*;
use rinch_core::use_store;
use rinch_core::Signal;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::{
    BetaBookmark, BetaChapterSummary, BetaFeedback, BetaReaderView, Book, Chapter,
    CreateBetaFeedbackRequest, CreateBetaReplyRequest, CreateBookmarkRequest,
    UpdateReadingProgressRequest,
};

use crate::api;
use crate::fonts;
use crate::pages::editor_utils;
use crate::router;
use crate::store::{AppStore, Route};

const READER_CSS: &str = r#"
/* ── Reader — its own quieter language ──────────────────────────────────────
   Shares tokens with the studio (colour, radius, spacing, motion) but none of
   its layout habits: no permanent sidebar, no permanent feedback rail. A beta
   reader navigates twice at most — pick a chapter, leave a note — so neither
   earns permanent furniture. Both surface on demand from the folio/header. */

.reader-workspace {
    display: flex;
    flex-direction: column;
    height: 100dvh;
    overflow: hidden;
    position: relative;
    font-family: var(--pw-font-ui);
}

/* ── Header — quiet, low-contrast metadata, not a toolbar ─────────────────── */
.reader-topbar {
    display: flex;
    align-items: center;
    gap: var(--pw-space-sm);
    padding: var(--pw-space-sm) var(--pw-space-lg);
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    border-bottom: 1px solid var(--pw-hairline);
    flex-shrink: 0;
    transition: opacity var(--pw-dur-slow) var(--pw-ease);
}

.reader-topbar-book {
    font-family: var(--pw-font-display);
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
}

.reader-topbar-sp { flex: 1; }

/* Mobile topbar mirrors the desktop one's quiet tone but keeps room for
   thumb-sized tap targets either side of the title. */
.reader-mobile-topbar {
    display: none;
    align-items: center;
    justify-content: space-between;
    gap: var(--pw-space-xs);
    padding: var(--pw-space-xs) var(--pw-space-sm);
    border-bottom: 1px solid var(--pw-hairline);
    flex-shrink: 0;
}

/* ── Reading column ─────────────────────────────────────────────────────── */
.reader-reading-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
    min-height: 0;
    position: relative;
}

.reader-viewport {
    flex: 1;
    overflow: hidden;
    position: relative;
    display: flex;
    justify-content: center;
    background: var(--rinch-color-body);
    touch-action: pan-y;
}

/* The fixed window that clips exactly one page. Vertical reading margins live
   here (consistent per page); horizontal margins are applied to the columns
   element in JS so every page is inset symmetrically.

   `max-width` is the measure token (`--pw-measure`, app_shell.rs), not a
   layout-pane width — this governs line length, not how much of the pane the
   reader occupies. It used to hardcode 760px (~85 characters/line at 17px,
   measured empirically) — wider than the editor ever was, and well past the
   65-75 char comfortable band. `--pw-measure` (37em) measures to ~72 at 17px. */
.reader-page-frame {
    width: 100%;
    max-width: var(--pw-measure);
    height: 100%;
    padding: var(--pw-space-2xl) 0;
    box-sizing: border-box;
    overflow: hidden;
    position: relative;
}

/* Margin page-turn hit targets. ~64px wide down both edges, nearly invisible
   until hovered — the gesture already exists as arrow keys and swipe, so the
   visible control doesn't need to compete with the prose for attention. */
.reader-turn {
    position: absolute;
    top: 0; bottom: 0;
    width: 64px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--rinch-color-placeholder);
    cursor: pointer;
    font-size: 22px;
    opacity: 0.35;
    z-index: 2;
    transition: opacity var(--pw-dur-fast) var(--pw-ease), background var(--pw-dur-fast) var(--pw-ease);
    user-select: none;
}

.reader-turn:hover {
    opacity: 1;
    background: var(--pw-hairline);
}

.reader-turn.l { left: 0; }
.reader-turn.r { right: 0; }
.reader-turn.disabled { visibility: hidden; }

.reader-content {
    height: 100%;
    box-sizing: border-box;
    font-family: var(--pw-font-prose);
    font-size: 17px;
    line-height: 1.85;
    color: var(--rinch-color-text);
    -webkit-font-smoothing: antialiased;
    user-select: text;
    /* Ragged right, NOT justified. Tried justified-with-hyphenation in the
       mockup and reverted it: at this measure, in a handwriting face, it
       opens obvious rivers between words. Revisit only if the prose face
       ever changes to something more justification-tolerant. */
    text-align: left;
    /* Multi-column pagination: column-width / column-gap / horizontal padding
       are set imperatively once content dimensions are known. */
    column-fill: auto;
    transition: transform 0.28s ease;
    will-change: transform;
}

.reader-content p { margin: 0 0 16px 0; }

/* Centred and right-aligned paragraphs drop the first-line indent the
   Typography pane's `paragraph_indent` otherwise puts on every `.reader-content p`
   — an indent would push the first line off the axis the alignment established.
   Justified keeps it: a justified paragraph is an ordinary indented paragraph
   whose lines are stretched. Mirrors the editor rule in EDITOR_CSS; see the long
   note there for why both spellings of the declaration are matched. */
.reader-content p[style*="text-align: center"],
.reader-content p[style*="text-align:center"],
.reader-content p[style*="text-align: right"],
.reader-content p[style*="text-align:right"] {
    text-indent: 0;
}
.reader-content h1, .reader-content h2, .reader-content h3 {
    font-family: var(--pw-font-display);
    font-weight: 400;
    line-height: 1.2;
}
.reader-content h1 { font-size: 1.8em; margin-bottom: 22px; }
.reader-content h2 { font-size: 1.55em; margin-bottom: 22px; }
.reader-content h3 { font-size: 1.3em; margin-bottom: 18px; }
.reader-content blockquote {
    border-left: 2px solid var(--rinch-color-teal-8);
    padding-left: var(--pw-space-md);
    margin: 16px 0;
    color: var(--rinch-color-dimmed);
}
.reader-content img {
    max-width: 100%;
    height: auto;
    border-radius: var(--pw-radius-sm);
    margin: 16px 0;
    display: block;
}
.reader-content strong { font-weight: 700; }
.reader-content em { font-style: italic; }

/* Existing feedback surfaces as a subtle underline in the prose, not a card in
   a rail: a linear-gradient underline (not a highlight block) so it reads as
   ink, not UI. Click opens the note where the words are. */
.reader-content mark.rd-mark {
    background: linear-gradient(transparent 84%, var(--rinch-color-teal-8) 84%);
    color: inherit;
    cursor: pointer;
    padding: 0;
    transition: background var(--pw-dur-fast) var(--pw-ease);
}

.reader-content mark.rd-mark:hover {
    background: linear-gradient(transparent 84%, var(--rinch-color-teal-6) 84%);
}

/* ── Folio — replaces the bordered page bar ────────────────────────────────
   A 2px progress line, then chapter / page / position-in-book at
   `--pw-text-2xs`, letterspaced, in the placeholder colour. No chevrons, no
   border, no "1 / 1" box — the page turns live in the margins instead. */
.reader-folio {
    padding: 10px var(--pw-space-lg) 14px;
    flex-shrink: 0;
    transition: opacity var(--pw-dur-slow) var(--pw-ease);
}

.reader-folio-progress {
    height: 2px;
    background: var(--pw-hairline);
    border-radius: var(--pw-radius-full);
    overflow: hidden;
    margin-bottom: 8px;
}

.reader-folio-progress i {
    display: block;
    height: 100%;
    background: var(--rinch-color-teal-7);
    transition: width var(--pw-dur) var(--pw-ease);
}

.reader-folio-row {
    display: flex;
    align-items: center;
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-placeholder);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    font-variant-numeric: tabular-nums;
    user-select: none;
}

.reader-folio-row .reader-topbar-sp { flex: 1; }

/* ── Feedback popover — replaces the fixed-position tooltip's old look.
   Anchored near the clicked mark/selection; flip-above is applied via an
   inline `top` computed in JS/Rust from the anchor + popover height. ────── */
.feedback-tooltip {
    position: fixed;
    width: 280px;
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-md);
    box-shadow: var(--pw-shadow-3);
    padding: var(--pw-space-sm);
    z-index: var(--pw-z-popover);
    display: none;
}

.feedback-tooltip.visible {
    display: block;
}

.feedback-tooltip-quote {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    font-style: italic;
    border-left: 2px solid var(--rinch-color-teal-8);
    padding-left: var(--pw-space-xs);
    margin-bottom: var(--pw-space-xs);
}

.feedback-tooltip-comment {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
    margin-bottom: var(--pw-space-xs);
    word-break: break-word;
}

.feedback-tooltip textarea {
    width: 100%;
    min-height: 52px;
    padding: 6px 8px;
    border: 1px solid var(--rinch-color-border);
    border-radius: var(--pw-radius-sm);
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    font-size: var(--pw-text-sm);
    font-family: inherit;
    resize: vertical;
    outline: none;
}

.feedback-tooltip textarea:focus {
    border-color: var(--rinch-color-teal-7);
}

.feedback-tooltip-actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: var(--pw-space-xs);
}

.feedback-tooltip-replies {
    margin-top: var(--pw-space-xs);
    padding-top: 6px;
    border-top: 1px solid var(--pw-hairline);
    max-height: 120px;
    overflow-y: auto;
}

.feedback-reply {
    padding: 4px 0;
    font-size: var(--pw-text-xs);
}

.feedback-reply-author {
    font-weight: 600;
    color: var(--rinch-color-teal-4);
}

.feedback-reply-author.owner {
    color: var(--rinch-color-teal-3);
}

/* ── Contents panel — on demand, opened from the header ────────────────────
   Slides in from the left over the reading column (not a full-app Sheet,
   which is a right-hand drawer reserved for multi-step flows). Carries the
   reader's own notes at the foot, which is what a reader wants to return to
   and previously had nowhere to see. */
.reader-contents-backdrop { display: none; }

.reader-contents {
    display: none;
    position: absolute;
    left: 0; top: 0; bottom: 0;
    width: 260px;
    background: var(--pw-color-deep);
    border-right: 1px solid var(--rinch-color-border);
    box-shadow: var(--pw-shadow-3);
    z-index: var(--pw-z-sheet);
    flex-direction: column;
    overflow: hidden;
}

.reader-contents.open { display: flex; }

.reader-contents-cover {
    width: 100%;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    display: block;
    border-bottom: 1px solid var(--rinch-color-border);
}

.reader-contents-title {
    padding: var(--pw-space-md) var(--pw-space-md) var(--pw-space-xs);
    font-family: var(--pw-font-display);
    font-size: var(--pw-text-lg);
    font-weight: 400;
    color: var(--rinch-color-text);
}

.reader-contents-meta {
    padding: 0 var(--pw-space-md) var(--pw-space-sm);
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    border-bottom: 1px solid var(--pw-hairline);
}

.reader-contents-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--pw-space-sm) var(--pw-space-md) var(--pw-space-2xs);
    font-size: var(--pw-text-2xs);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--rinch-color-placeholder);
}

.reader-contents-chapters {
    overflow-y: auto;
    padding: var(--pw-space-2xs) 0;
}

.reader-chapter-item {
    padding: var(--pw-space-xs) var(--pw-space-md);
    cursor: pointer;
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
    transition: background var(--pw-dur-fast) var(--pw-ease);
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
}

.reader-chapter-item:hover {
    background: var(--rinch-color-surface);
}

.reader-chapter-item.active {
    background: var(--rinch-color-surface);
    color: var(--rinch-color-teal-4);
    font-weight: 600;
}

.reader-chapter-num {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    min-width: 20px;
}

/* Bookmarks + the reader's own notes share the footer of the contents panel. */
.reader-contents-footer {
    border-top: 1px solid var(--pw-hairline);
    padding: var(--pw-space-xs) 0;
    max-height: 40%;
    overflow-y: auto;
    flex-shrink: 0;
}

.reader-bookmark-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: var(--pw-space-2xs) var(--pw-space-xs) var(--pw-space-2xs) var(--pw-space-md);
    font-size: var(--pw-text-sm);
}

.reader-bookmark-item:hover {
    background: var(--rinch-color-surface);
}

.reader-bookmark-label {
    flex: 1;
    cursor: pointer;
    color: var(--rinch-color-teal-4);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.reader-note-item {
    padding: var(--pw-space-2xs) var(--pw-space-md);
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
    font-style: italic;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.reader-note-item:hover {
    background: var(--rinch-color-surface);
    color: var(--rinch-color-text);
}

.reader-contents-back {
    padding: var(--pw-space-xs) var(--pw-space-sm);
    border-top: 1px solid var(--pw-hairline);
}

/* ── Welcome / error states ─────────────────────────────────────────────── */
.reader-welcome {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    text-align: center;
    padding: 40px;
}

.reader-welcome h2 {
    font-family: var(--pw-font-display);
    font-weight: 400;
    color: var(--rinch-color-teal-4);
    margin-bottom: 8px;
}

.reader-error {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100dvh;
    text-align: center;
    padding: 40px;
}

.reader-error h2 {
    font-family: var(--pw-font-display);
    font-weight: 400;
    color: var(--rinch-color-teal-4);
    margin-bottom: 8px;
}

@media (max-width: 768px) {
    .reader-mobile-topbar { display: flex; }
    .reader-topbar { display: none; }
    .reader-page-frame { padding: var(--pw-space-lg) 0; }
    .reader-turn { width: 44px; }

    .reader-contents {
        width: 82vw;
        max-width: 320px;
        border-radius: 0 var(--pw-radius-lg) var(--pw-radius-lg) 0;
    }
    .reader-contents-backdrop.open {
        display: block;
        position: fixed; top: 0; left: 0; right: 0; bottom: 0;
        background: rgba(0,0,0,0.4);
        z-index: calc(var(--pw-z-sheet) - 1);
    }

    /* Tooltip/popover as a bottom sheet on mobile — same precedent as the
       editor's other mobile overlays. */
    .feedback-tooltip.visible {
        left: 0 !important; right: 0 !important;
        top: auto !important; bottom: 0 !important;
        width: 100%;
        border-radius: var(--pw-radius-lg) var(--pw-radius-lg) 0 0;
        padding: var(--pw-space-md);
    }
}
"#;

/// Where the reader gets its data. Beta readers hit the token-scoped public
/// endpoints (with feedback + progress persistence); author preview reads the
/// authenticated book/chapter endpoints and never writes anything back.
#[derive(Clone)]
enum ReaderSource {
    Beta(String),          // beta link token
    AuthorPreview(String), // book_id
}

/// Horizontal reading margin (px) for the paginated column, by viewport width.
fn side_pad(vw: f64) -> f64 {
    if vw < 640.0 { 18.0 } else { 48.0 }
}

fn reader_content_el() -> Option<web_sys::HtmlElement> {
    crate::platform::window()?
        .document()?
        .query_selector("#reader-content")
        .ok()
        .flatten()?
        .dyn_into()
        .ok()
}

/// Build the paginated reading column and publish its handle into `content_el`.
///
/// `#reader-content` lives inside an rsx `if` (a `show_dom` branch), so the node
/// only exists once `current_chapter` flips to `Some`. Publishing the handle from
/// the branch render hands `open_chapter` the element directly — no
/// `query_selector`, and nothing to wait for. `show_dom` only re-renders a branch
/// when the *condition* flips, so this handle stays valid across chapter switches
/// (and is republished if the branch is ever rebuilt).
///
/// The `id` stays: pagination measuring, the beta feedback text-walk (`closest`)
/// and the e2e specs all select `#reader-content`. It is simply no longer the way
/// content gets *in*.
fn reader_content_node(
    __scope: &mut RenderScope,
    content_el: Signal<Option<NodeHandle>>,
) -> NodeHandle {
    let el = rsx! {
        div {
            class: "reader-content",
            id: "reader-content",
        }
    };
    content_el.set(Some(el.clone()));
    el
}

/// Apply the multi-column styling to `#reader-content` (sized to the live
/// viewport) and return the resulting total page count. Reading `scroll_width`
/// forces the synchronous reflow we need before counting pages.
// Web-only: pagination is measured out of the DOM's multi-column layout.
#[cfg(target_arch = "wasm32")]
fn measure_and_style() -> i32 {
    let Some(el) = reader_content_el() else { return 1 };
    let vw = el.client_width() as f64;
    if vw <= 1.0 {
        return 1;
    }
    let pad = side_pad(vw);
    let col_w = (vw - 2.0 * pad).max(1.0);
    let style = el.style();
    let _ = style.set_property("column-width", &format!("{}px", col_w));
    let _ = style.set_property("column-gap", &format!("{}px", pad));
    let _ = style.set_property("padding-left", &format!("{}px", pad));
    let _ = style.set_property("padding-right", &format!("{}px", pad));
    let stride = vw - pad; // = col_w + gap
    let sw = el.scroll_width() as f64;
    // scroll_width == pad + n * stride, so (sw - pad) / stride == n.
    (((sw - pad) / stride) - 0.01).ceil().max(1.0) as i32
}

/// Translate the columns element so `page` is the visible page.
fn apply_page_transform(page: i32) {
    if let Some(el) = reader_content_el() {
        let vw = el.client_width() as f64;
        let pad = side_pad(vw);
        let stride = (vw - pad).max(0.0);
        let _ = el
            .style()
            .set_property("transform", &format!("translateX(-{}px)", (page as f64) * stride));
    }
}

/// Compute where to anchor the feedback popover/tooltip given the clicked/
/// selected rect (`anchor`, viewport coordinates) and the reading viewport's
/// own rect (`frame`, the scroll/clip boundary the popover must stay inside).
///
/// Mirrors the mockup's flip logic (`design/02-screens.html`'s `rdmark` click
/// handler): open below the anchor by default; flip above it when the popover
/// (of `box_height`) wouldn't fit before the bottom of the frame. This matters
/// most at the bottom of a page, where "below" often means "off-screen."
/// `box_height` is an estimate (the box hasn't rendered yet at the moment we
/// decide where to place it) — 160px covers the compose tooltip and a
/// popover with a couple of short replies; taller popovers still clamp to the
/// viewport via the caller's own `max-height`/scroll, they just may sit
/// slightly higher than ideal.
///
/// Pure arithmetic (no DOM) so it compiles on both targets without a
/// `web_only!` wrapper — callers that feed it real rects are themselves
/// web-only (there is no `Selection`/`getBoundingClientRect` on native), but
/// the function itself has nothing target-specific to gate.
fn compute_popover_position(
    anchor_left: f64,
    anchor_top: f64,
    anchor_bottom: f64,
    frame_top: f64,
    frame_bottom: f64,
    box_height: f64,
) -> (f64, f64, bool) {
    let below = anchor_bottom + 10.0;
    let fits_below = below + box_height <= frame_bottom - 12.0;
    if fits_below {
        (anchor_left, below, false)
    } else {
        let above = (anchor_top - box_height - 10.0).max(frame_top + 8.0);
        (anchor_left, above, true)
    }
}

/// True if the current document selection is empty (used so a text-selection
/// drag for feedback isn't mistaken for a page swipe).
// Web-only: reads the DOM selection; only the (web-only) swipe pager needs it.
#[cfg(target_arch = "wasm32")]
fn selection_is_empty() -> bool {
    crate::platform::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_selection().ok().flatten())
        .map(|s| s.to_string().as_string().unwrap_or_default().trim().is_empty())
        .unwrap_or(true)
}

/// The id of the chapter `delta` positions from the active one in reading order
/// (`-1` = previous, `+1` = next), or `None` at the ends. Lets paging past a
/// chapter boundary flip into the adjacent chapter.
fn adjacent_chapter_id(
    view_data: Signal<Option<BetaReaderView>>,
    active_chapter_id: Signal<Option<String>>,
    delta: i32,
) -> Option<String> {
    let view = view_data.get()?;
    let current = active_chapter_id.get()?;
    let idx = view.chapters.iter().position(|c| c.id == current)?;
    let target = idx as i32 + delta;
    if target < 0 || target as usize >= view.chapters.len() {
        return None;
    }
    Some(view.chapters[target as usize].id.clone())
}

// ── Inline feedback marks ───────────────────────────────────────────────────
//
// Existing feedback surfaces as a `.rd-mark` underline in the prose rather
// than a card in a rail (the rail is gone). After a chapter's HTML is
// injected, `apply_feedback_marks` walks `#reader-content`'s text nodes once
// per feedback item, finds `selected_text` (disambiguated by `context_block`
// when it recurs), and wraps the match in a `<mark class="rd-mark">` wired to
// `on_mark_click`. This is a leaner cousin of `scroll_to_text_in_editor`
// (`pages/book/panes/editor.rs`): that function scrolls to *one* target and
// flashes it; this one permanently wraps every feedback item's quote (one
// text-node walk per item, since wrapping mutates the DOM out from under a
// shared node list) and does not scroll or flash anything.
//
// Web-only: there is no DOM to walk on native.
#[cfg(target_arch = "wasm32")]
mod feedback_marks {
    use wasm_bindgen::JsCast;

    /// Collapse runs of whitespace into a single space and trim (mirrors the
    /// editor's copy in `book/panes/editor.rs` — kept local rather than shared
    /// across a module boundary for two call sites with different node-walk
    /// needs).
    fn normalize_whitespace(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut last_was_space = true;
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

    fn utf8_byte_to_utf16(text: &str, byte_offset: usize) -> u32 {
        let clamped = byte_offset.min(text.len());
        text[..clamped].encode_utf16().count() as u32
    }

    /// Like `normalize_whitespace`, but also returns a mapping from each byte
    /// offset in the normalized output back to the corresponding byte offset in
    /// the original input — needed because matches are found in normalized text
    /// but `wrap_range` must operate on offsets into the raw DOM-collected text.
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
        mapping.push(s.len());
        (result, mapping)
    }

    /// Recursively collect text nodes under `node`, inserting a space at
    /// block-element boundaries (so cross-paragraph quotes still match) and
    /// skipping anything already inside a `mark.rd-mark` (so a second pass
    /// never double-wraps).
    fn collect_text_nodes(
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
        if let Ok(el) = node.clone().dyn_into::<web_sys::Element>() {
            if el.tag_name().eq_ignore_ascii_case("mark") {
                return;
            }
            let tag = el.tag_name().to_lowercase();
            let is_block = matches!(
                tag.as_str(),
                "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li" | "br"
            );
            if is_block && !full_text.is_empty() && !full_text.ends_with(' ') {
                full_text.push(' ');
            }
        }
        let children = node.child_nodes();
        for i in 0..children.length() {
            if let Some(child) = children.item(i) {
                collect_text_nodes(&child, text_nodes, full_text, node_offsets);
            }
        }
    }

    fn byte_offset_to_node(node_offsets: &[(usize, usize)], byte_offset: usize) -> Option<(usize, usize)> {
        node_offsets
            .iter()
            .position(|&(start, end)| byte_offset >= start && byte_offset <= end)
            .map(|i| (i, byte_offset - node_offsets[i].0))
    }

    /// Find the best-matching byte range for `needle` (normalized) in `haystack`
    /// (already normalized), disambiguating multiple hits with `context`. Mirrors
    /// the editor's disambiguation but without the fuzzy-prefix fallback — a
    /// feedback quote that no longer appears verbatim (the author edited the
    /// prose) simply isn't marked, rather than guessing at a partial match.
    fn find_best_match(haystack: &str, needle: &str, context: &str) -> Option<(usize, usize)> {
        if needle.is_empty() {
            return None;
        }
        let mut matches = Vec::new();
        let mut search_start = 0;
        while let Some(pos) = haystack[search_start..].find(needle) {
            matches.push(search_start + pos);
            search_start += pos + 1;
        }
        if matches.is_empty() {
            return None;
        }
        let best = if matches.len() == 1 || context.is_empty() {
            matches[0]
        } else {
            *matches.iter().min_by_key(|&&pos| {
                let ctx_start = pos.saturating_sub(context.len());
                let ctx_end = (pos + needle.len() + context.len()).min(haystack.len());
                let surrounding = &haystack[ctx_start..ctx_end];
                let overlap = longest_common_substring(surrounding, context);
                context.len().saturating_sub(overlap)
            }).unwrap()
        };
        Some((best, best + needle.len()))
    }

    fn longest_common_substring(a: &str, b: &str) -> usize {
        let a_bytes = a.as_bytes();
        let b_bytes = b.as_bytes();
        let mut max_len = 0;
        let mut prev = vec![0usize; b_bytes.len() + 1];
        let mut curr = vec![0usize; b_bytes.len() + 1];
        for i in 1..=a_bytes.len() {
            for j in 1..=b_bytes.len() {
                if a_bytes[i - 1] == b_bytes[j - 1] {
                    curr[j] = prev[j - 1] + 1;
                    max_len = max_len.max(curr[j]);
                } else {
                    curr[j] = 0;
                }
            }
            std::mem::swap(&mut prev, &mut curr);
            curr.iter_mut().for_each(|v| *v = 0);
        }
        max_len
    }

    /// Wrap the byte range `[start, end)` of the raw text collected by
    /// `collect_text_nodes` (i.e. offsets into `full_text`/`node_offsets`, not
    /// the whitespace-normalized text matches are found against — callers map
    /// back first) in one or more `<mark class="rd-mark" data-feedback-id>`
    /// elements inside the live DOM. A quote spanning multiple text nodes (e.g.
    /// crossing inline `<em>`/`<strong>` boundaries) gets one mark per covered
    /// node, all sharing the same id; the first is returned for the caller to
    /// attach a click listener / read a bounding rect from.
    fn wrap_range(
        doc: &web_sys::Document,
        text_nodes: &[web_sys::Node],
        node_offsets: &[(usize, usize)],
        start: usize,
        end: usize,
        feedback_id: &str,
    ) -> Option<web_sys::Element> {
        let (start_idx, start_off) = byte_offset_to_node(node_offsets, start)?;
        let (end_idx, end_off) = byte_offset_to_node(node_offsets, end)?;
        let mut first_mark = None;
        for idx in start_idx..=end_idx {
            if idx >= text_nodes.len() {
                break;
            }
            let node = &text_nodes[idx];
            let text = node.text_content().unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            let node_u16_len = text.encode_utf16().count() as u32;
            let slice_start_u16 = if idx == start_idx { utf8_byte_to_utf16(&text, start_off) } else { 0 };
            let slice_end_u16 = if idx == end_idx { utf8_byte_to_utf16(&text, end_off) } else { node_u16_len };
            if slice_start_u16 >= slice_end_u16 {
                continue;
            }

            let target_node = if slice_start_u16 == 0 && slice_end_u16 == node_u16_len {
                Some(node.clone())
            } else if let Ok(text_node) = node.clone().dyn_into::<web_sys::Text>() {
                let _after = text_node.split_text(slice_end_u16).ok();
                text_node.split_text(slice_start_u16).ok().map(|t| t.into())
            } else {
                None
            };

            if let Some(target) = target_node {
                if let Ok(mark) = doc.create_element("mark") {
                    mark.set_class_name("rd-mark");
                    mark.set_attribute("data-feedback-id", feedback_id).ok();
                    if let Some(parent) = target.parent_node() {
                        parent.insert_before(&mark, Some(&target)).ok();
                        mark.append_child(&target).ok();
                        if first_mark.is_none() {
                            first_mark = Some(mark);
                        }
                    }
                }
            }
        }
        first_mark
    }

    /// Wrap each feedback item's quoted text in `#reader-content` with a
    /// clickable `<mark class="rd-mark" data-feedback-id="...">`, invoking
    /// `on_click(feedback_id, mark_element)` when one is clicked. Call after
    /// every fresh `set_inner_html` of the chapter content (a chapter switch
    /// replaces the whole subtree, so previous marks don't need explicit
    /// teardown — they're discarded with the old HTML).
    pub fn apply(
        feedback: &[plotweb_common::BetaFeedback],
        on_click: impl Fn(String, web_sys::Element) + 'static + Copy,
    ) {
        let Some(doc) = crate::platform::window().and_then(|w| w.document()) else { return };
        let Some(root) = doc.query_selector("#reader-content").ok().flatten() else { return };

        for fb in feedback {
            if fb.selected_text.trim().is_empty() {
                continue;
            }
            // Re-walk per feedback item: each successful wrap mutates the DOM
            // (splits/re-parents text nodes), which would invalidate a
            // previously-collected node list for the next item.
            let mut text_nodes = Vec::new();
            let mut full_text = String::new();
            let mut node_offsets = Vec::new();
            collect_text_nodes(&root.clone().into(), &mut text_nodes, &mut full_text, &mut node_offsets);

            // Matches are found in whitespace-normalized text; map the match back
            // to raw byte offsets (into `full_text`, which `node_offsets` indexes)
            // via `norm_to_raw` before wrapping.
            let (norm_text, norm_to_raw) = normalize_whitespace_map(&full_text);
            let norm_needle = normalize_whitespace(&fb.selected_text);
            let norm_context = normalize_whitespace(&fb.context_block);
            let Some((norm_start, norm_end)) = find_best_match(&norm_text, &norm_needle, &norm_context) else {
                continue;
            };
            let start = norm_to_raw[norm_start];
            let end = if norm_end < norm_to_raw.len() { norm_to_raw[norm_end] } else { full_text.len() };

            let fb_id = fb.id.clone();
            if let Some(mark) = wrap_range(&doc, &text_nodes, &node_offsets, start, end, &fb_id) {
                let mark_for_closure = mark.clone();
                let id_for_closure = fb_id.clone();
                let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
                    event.stop_propagation();
                    on_click(id_for_closure.clone(), mark_for_closure.clone());
                }) as Box<dyn FnMut(_)>);
                mark.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref()).ok();
                closure.forget();
            }
        }
    }
}

fn reader_bookmark_item<F, D, DO>(
    __scope: &mut RenderScope,
    bm: BetaBookmark,
    open: F,
    del: D,
) -> NodeHandle
where
    F: Fn(String, i32) + 'static + Copy,
    D: Fn(String) -> DO + 'static + Copy,
    DO: Fn() + 'static,
{
    let cid = bm.chapter_id.clone();
    let page = bm.page as i32;
    let label = bm.label.clone();
    let del_id = bm.id.clone();
    rsx! {
        div { class: "reader-bookmark-item", key: bm.id,
            div {
                class: "reader-bookmark-label",
                onclick: move || open(cid.clone(), page),
                {label}
            }
            ActionIcon {
                variant: "subtle",
                size: "xs",
                color: "red",
                onclick: del(del_id),
                {render_tabler_icon(__scope, TablerIcon::Trash, TablerIconStyle::Outline)}
            }
        }
    }
}

/// One row in the Contents panel's "Your notes" section: a truncated quote
/// (the tail of `selected_text`, since a reader recognizes a line by how it
/// ends more often than how it starts) that opens the note's chapter and
/// pops its feedback popover — this is "return to a note I left," the thing
/// the mockup calls out as having nowhere to live before this pass.
fn reader_note_item<F>(
    __scope: &mut RenderScope,
    fb: BetaFeedback,
    contents_open: Signal<bool>,
    popover_feedback_id: Signal<Option<String>>,
    open_chapter: F,
) -> NodeHandle
where
    F: Fn(String, i32) + 'static + Copy,
{
    let cid = fb.chapter_id.clone();
    let fid = fb.id.clone();
    let quote_tail: String = if fb.selected_text.chars().count() > 60 {
        fb.selected_text.chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect()
    } else {
        fb.selected_text.clone()
    };
    let quote = format!("\u{201c}\u{2026}{}\u{201d}", quote_tail);
    rsx! {
        div {
            class: "reader-note-item",
            key: fb.id.clone(),
            onclick: move || {
                contents_open.set(false);
                open_chapter(cid.clone(), 0);
                popover_feedback_id.set(Some(fid.clone()));
            },
            {quote}
        }
    }
}

fn reader_chapter_item<F, FO>(
    __scope: &mut RenderScope,
    ch_id: String,
    ch_title: String,
    ch_sort: i64,
    active_chapter_id: Signal<Option<String>>,
    load_chapter: F,
) -> NodeHandle
where
    F: Fn(String) -> FO + 'static + Copy,
    FO: Fn() + 'static,
{
    let cid = std::rc::Rc::new(ch_id.clone());
    rsx! {
        div {
            class: {
                let cid = cid.clone();
                move || if active_chapter_id.get().as_deref() == Some(cid.as_str()) {
                    "reader-chapter-item active"
                } else {
                    "reader-chapter-item"
                }
            },
            onclick: load_chapter(ch_id),
            span { class: "reader-chapter-num",
                {format!("{}.", ch_sort + 1)}
            }
            {ch_title}
        }
    }
}

/// The popover body for an *existing* feedback item, opened by clicking its
/// `.rd-mark` in the prose (replaces the old permanent feedback-rail card —
/// see `reader-mockup`'s `.rd-pop`). Shows the quote, the original comment,
/// any replies, and a reply box; reuses the `feedback-tooltip*` classes so it
/// shares chrome with the "leave new feedback" tooltip below.
fn reader_feedback_popover<F, FO>(
    __scope: &mut RenderScope,
    fb: BetaFeedback,
    reply_drafts: Signal<std::collections::HashMap<String, String>>,
    reply_to_feedback: F,
) -> NodeHandle
where
    F: Fn(String) -> FO + 'static + Copy,
    FO: Fn() + 'static,
{
    let fb_id2 = fb.id.clone();
    let fb_id3 = fb.id.clone();
    let fb_id_value = std::rc::Rc::new(fb.id.clone());
    let fb_id_input = fb.id.clone();
    let fb_id_enter = fb.id.clone();
    let fb_comment = fb.comment.clone();
    let quote_text = format!("\u{201c}{}\u{201d}", fb.selected_text);
    let reply_nodes: Vec<NodeHandle> = fb.replies.iter().map(|r| {
        reply_item(__scope, r.author_type.clone(), r.author_name.clone(), r.content.clone())
    }).collect();

    let reply_submit_id = __scope.register_handler(reply_to_feedback(fb_id_enter));
    let reply_box = rsx! {
        div { class: "feedback-reply-input",
            textarea {
                id: {format!("reply-input-{}", fb_id2)},
                placeholder: "Reply...",
                rows: "1",
                value: {
                    let k = fb_id_value.clone();
                    move || reply_drafts.get().get(k.as_str()).cloned().unwrap_or_default()
                },
                oninput: move |v: String| {
                    let key = fb_id_input.clone();
                    reply_drafts.update(|m| { m.insert(key, v); });
                },
            }
            ActionIcon {
                variant: "subtle",
                size: "xs",
                onclick: reply_to_feedback(fb_id3),
                {render_tabler_icon(__scope, TablerIcon::Send, TablerIconStyle::Outline)}
            }
        }
    };
    reply_box.set_attribute("data-onsubmit", &reply_submit_id.0.to_string());

    let has_comment = !fb_comment.is_empty();
    let has_replies = !reply_nodes.is_empty();
    rsx! {
        Fragment {
            div { class: "feedback-tooltip-quote", {quote_text} }
            if has_comment {
                div { class: "feedback-tooltip-comment", {fb_comment.clone()} }
            }
            if has_replies {
                div { class: "feedback-tooltip-replies",
                    {reply_nodes.clone()}
                }
            }
            {reply_box}
        }
    }
}

fn reply_item(
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

#[component]
pub fn reader_page(token: String) -> NodeHandle {
    reader_body(__scope, ReaderSource::Beta(token))
}

#[component]
pub fn reader_preview_page(book_id: String) -> NodeHandle {
    reader_body(__scope, ReaderSource::AuthorPreview(book_id))
}

/// Shared reader implementation for both beta readers and author preview.
fn reader_body(__scope: &mut RenderScope, source: ReaderSource) -> NodeHandle {
    let store = use_store::<AppStore>();

    let is_preview = matches!(source, ReaderSource::AuthorPreview(_));
    let (token, book_id) = match &source {
        ReaderSource::Beta(t) => (t.clone(), String::new()),
        ReaderSource::AuthorPreview(b) => (String::new(), b.clone()),
    };
    let token_signal = Signal::new(token.clone());
    let book_id_signal = Signal::new(book_id.clone());

    let view_data: Signal<Option<BetaReaderView>> = Signal::new(None);
    let current_chapter: Signal<Option<Chapter>> = Signal::new(None);
    let active_chapter_id: Signal<Option<String>> = Signal::new(None);
    let feedback_list: Signal<Vec<BetaFeedback>> = Signal::new(Vec::new());
    let bookmarks: Signal<Vec<BetaBookmark>> = Signal::new(Vec::new());
    // Contents panel: chapters + this reader's own notes, opened on demand from
    // the header (replaces the old always-mounted 260px sidebar). Shared between
    // desktop and mobile — there is only one on-demand panel now, not a
    // desktop sidebar plus a separate mobile drawer.
    let contents_open: Signal<bool> = Signal::new(false);
    let error_msg: Signal<Option<String>> = Signal::new(None);

    // Pagination state
    let current_page: Signal<i32> = Signal::new(0);
    let total_pages: Signal<i32> = Signal::new(1);
    // Handle to the `#reader-content` element, published by `reader_content_node`
    // when the chapter branch renders. Chapter HTML is injected through this.
    let content_el: Signal<Option<NodeHandle>> = Signal::new(None);
    // Debounce handle for progress saves (a window timeout id, or None).
    let progress_timer: Signal<Option<i32>> = Signal::new(None);

    // ── Feedback popover / compose-tooltip state ──────────────────────────────
    // The single floating `.feedback-tooltip` box does one of two jobs, never
    // both: composing new feedback on a fresh selection (`tooltip_visible`,
    // seeded from `tooltip_selected_text`/`tooltip_context`), or showing an
    // existing item's quote + replies after clicking its `.rd-mark`
    // (`popover_feedback_id`). Anchor position is shared (`popover_x/y`) since
    // both are "point at a spot in the prose" — only the content differs.
    // `compute_popover_position` already folds the flip-above decision into
    // `y` (it returns the box's final top, above or below), so there is no
    // separate "flipped" signal to read at render time.
    let tooltip_visible: Signal<bool> = Signal::new(false);
    let tooltip_selected_text: Signal<String> = Signal::new(String::new());
    let tooltip_context: Signal<String> = Signal::new(String::new());
    let tooltip_comment: Signal<String> = Signal::new(String::new());
    let popover_feedback_id: Signal<Option<String>> = Signal::new(None);
    // Anchor point (viewport px) the open box's top-left points at.
    let popover_x: Signal<f64> = Signal::new(0.0);
    let popover_y: Signal<f64> = Signal::new(0.0);

    // Per-feedback reply drafts, keyed by feedback id. The reply textareas are
    // controlled off this map (`value:` + `oninput:`) so submitting can read the
    // text from state instead of the DOM — the DOM read no-oped on native.
    let reply_drafts: Signal<std::collections::HashMap<String, String>> =
        Signal::new(std::collections::HashMap::new());

    // ── Auto last-page: persist reading position, debounced (beta only) ──────
    let save_progress = move |chapter_id: String, page: i32| {
        if is_preview {
            return;
        }
        let window = match crate::platform::window() {
            Some(w) => w,
            None => return,
        };
        if let Some(h) = progress_timer.get() {
            window.clear_timeout_with_handle(h);
        }
        let tok = token_signal.get();
        let closure = wasm_bindgen::closure::Closure::once(move || {
            let req = UpdateReadingProgressRequest { chapter_id, page: page as i64 };
            api::put::<_, serde_json::Value>(
                &format!("/api/beta/{}/progress", tok),
                &req,
                move |_result| {},
            );
        });
        let handle = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                500,
            )
            .unwrap_or(-1);
        closure.forget();
        progress_timer.set(Some(handle));
    };

    // ── Pagination: (re)measure the column layout and place a page ───────────
    // Deferred to a rAF because inner_html fills / reflows asynchronously.
    //
    // Native: inert. Pagination is measured out of the DOM's column layout
    // (`measure_and_style` / `apply_page_transform`), which has no native
    // equivalent yet, so desktop shows the chapter as one continuously-rendered
    // column. `total_pages` stays at its initialised 1 and `current_page` at 0 —
    // consistent with a single unpaginated page — so callers below still read
    // sane values.
    let repaginate = move |target_page: i32| {
        let _ = target_page;
        crate::web_only! {
            let closure = wasm_bindgen::closure::Closure::once(move || {
                let total = measure_and_style();
                total_pages.set(total);
                let clamped = target_page.max(0).min((total - 1).max(0));
                current_page.set(clamped);
                apply_page_transform(clamped);
                if let Some(cid) = active_chapter_id.get() {
                    save_progress(cid, clamped);
                }
            });
            if let Some(w) = crate::platform::window() {
                w.request_animation_frame(closure.as_ref().unchecked_ref()).ok();
            }
            closure.forget();
        }
    };

    // ── Open a chapter, optionally resuming at `target_page` ─────────────────
    // ── Open the popover for an existing feedback item, anchored to its mark ──
    // Shared by the click handler `feedback_marks::apply` attaches to each
    // `.rd-mark` (web-only — marks only exist once there's a DOM to click in).
    // Native: this closure is never invoked — it exists only as the click
    // handler `feedback_marks::apply` (web-only) attaches to each mark — but it
    // must still exist and type-check on native, since `refresh_marks`/
    // `open_chapter` name it inside their own `web_only!` blocks.
    let open_feedback_popover = move |feedback_id: String, anchor: web_sys::Element| {
        tooltip_visible.set(false);
        popover_feedback_id.set(Some(feedback_id));
        let rect = anchor.get_bounding_client_rect();
        let _ = &rect;
        crate::web_only! {
            if let Some(frame) = crate::platform::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector("#reader-viewport").ok().flatten())
            {
                let frame_rect = frame.get_bounding_client_rect();
                let (x, y, _flip) = compute_popover_position(
                    rect.left(), rect.top(), rect.bottom(),
                    frame_rect.top(), frame_rect.bottom(),
                    180.0,
                );
                popover_x.set(x);
                popover_y.set(y);
            }
        }
    };
    // Touch the binding on every target: its only callers (`feedback_marks::
    // apply`, below) are named inside `web_only!` blocks, which vanish
    // entirely from the native AST — without this, native sees the closure
    // itself as dead.
    let _ = &open_feedback_popover;

    // ── Open a chapter, optionally resuming at `target_page` ─────────────────
    let open_chapter = move |chapter_id: String, target_page: i32| {
        active_chapter_id.set(Some(chapter_id.clone()));
        tooltip_visible.set(false);
        popover_feedback_id.set(None);
        contents_open.set(false);
        current_page.set(0);
        total_pages.set(1);
        let cid = chapter_id.clone();
        let url = if is_preview {
            format!("/api/books/{}/chapters/{}", book_id_signal.get(), cid)
        } else {
            format!("/api/beta/{}/chapters/{}", token_signal.get(), cid)
        };
        api::get::<Chapter>(&url, move |result| {
            if let Ok(ch) = result {
                // Guard: a newer chapter may have been selected while this
                // request was in flight. Don't inject stale content.
                if active_chapter_id.get().as_deref() != Some(cid.as_str()) {
                    return;
                }
                current_chapter.set(Some(ch.clone()));
                let content_html = if ch.content.is_empty() {
                    "<p><em>This chapter is empty.</em></p>".to_string()
                } else {
                    editor_utils::content_to_display_html(&ch.content)
                };
                // `current_chapter.set` above synchronously renders the branch that
                // owns `#reader-content` (signal notification runs effects inline),
                // so the handle is published by the time we read it here. The stale
                // -chapter guard above already ran in this same callback, so no
                // second guard is needed.
                if let Some(el) = content_el.get() {
                    el.set_inner_html(&editor_utils::sanitize_html(&content_html));
                    // Wrap this chapter's feedback quotes in clickable marks
                    // before measuring columns, so pagination accounts for the
                    // final DOM (the wrap only changes text-node boundaries,
                    // not visible layout, but keeping the order avoids any
                    // doubt about it). Beta-only: preview has no feedback.
                    if !is_preview {
                        crate::web_only! {
                            let cid_for_marks = cid.clone();
                            let marks_for: Vec<BetaFeedback> = feedback_list.get()
                                .into_iter()
                                .filter(|f| f.chapter_id == cid_for_marks)
                                .collect();
                            feedback_marks::apply(&marks_for, open_feedback_popover);
                        }
                    }
                    repaginate(target_page);
                    // A second pass catches late reflow (e.g. images loading).
                    // Web-only: there is no reflow to catch without a DOM, and
                    // `repaginate` is already inert on native.
                    crate::web_only! {
                        let closure = wasm_bindgen::closure::Closure::once(move || {
                            repaginate(current_page.get());
                        });
                        if let Some(w) = crate::platform::window() {
                            w.set_timeout_with_callback_and_timeout_and_arguments_0(
                                closure.as_ref().unchecked_ref(),
                                250,
                            ).ok();
                        }
                        closure.forget();
                    }
                }
            }
        });
    };

    // Re-wrap `#reader-content`'s marks against the current `feedback_list` for
    // whichever chapter is open. Cheap to call after any change to the list
    // (submit, WS push, reply) since `feedback_marks::collect_text_nodes` skips
    // existing `mark.rd-mark` nodes — but a chapter switch already re-injects
    // the whole subtree via `open_chapter`, so this only needs to run when the
    // *list* changes underneath an already-open chapter, not on every render.
    let refresh_marks = move || {
        if is_preview {
            return;
        }
        let Some(cid) = active_chapter_id.get() else { return };
        // Native: inert — there is no DOM to wrap marks into. `cid` (and
        // `open_feedback_popover`, only ever consumed by `feedback_marks::apply`
        // below) are otherwise unused on that target.
        let _ = &cid;
        crate::web_only! {
            let marks_for: Vec<BetaFeedback> = feedback_list.get()
                .into_iter()
                .filter(|f| f.chapter_id == cid)
                .collect();
            feedback_marks::apply(&marks_for, open_feedback_popover);
        }
    };

    // ── Turn to a page; paging off either end flips to the adjacent chapter ───
    let go_to_page = move |page: i32| {
        let total = total_pages.get();
        // Past the last page → open the next chapter at its first page.
        if page > total - 1 {
            if let Some(next_id) = adjacent_chapter_id(view_data, active_chapter_id, 1) {
                open_chapter(next_id, 0);
            }
            return;
        }
        // Before the first page → open the previous chapter at its last page
        // (a large target page is clamped to the last by repaginate).
        if page < 0 {
            if let Some(prev_id) = adjacent_chapter_id(view_data, active_chapter_id, -1) {
                open_chapter(prev_id, i32::MAX);
            }
            return;
        }
        let clamped = page.max(0).min((total - 1).max(0));
        if clamped == current_page.get() {
            return;
        }
        // The prose translates under a stale popover/tooltip otherwise —
        // whatever it pointed at is no longer on-screen.
        tooltip_visible.set(false);
        popover_feedback_id.set(None);
        current_page.set(clamped);
        apply_page_transform(clamped);
        if let Some(cid) = active_chapter_id.get() {
            save_progress(cid, clamped);
        }
    };

    // ── Bookmarks (beta only) ────────────────────────────────────────────────
    let add_bookmark = move || {
        if is_preview {
            return;
        }
        let (Some(cid), Some(ch)) = (active_chapter_id.get(), current_chapter.get()) else {
            return;
        };
        let page = current_page.get();
        let label = format!("Ch. {} \u{b7} p.{}", ch.title, page + 1);
        let tok = token_signal.get();
        let req = CreateBookmarkRequest { chapter_id: cid, page: page as i64, label };
        api::post::<_, BetaBookmark>(
            &format!("/api/beta/{}/bookmarks", tok),
            &req,
            move |result| {
                if let Ok(bm) = result {
                    bookmarks.update(|list| list.push(bm));
                }
            },
        );
    };

    let delete_bookmark = move |id: String| {
        move || {
            if is_preview {
                return;
            }
            let tok = token_signal.get();
            let bid = id.clone();
            api::delete_req::<serde_json::Value>(
                &format!("/api/beta/{}/bookmarks/{}", tok, bid),
                move |result| {
                    if result.is_ok() {
                        bookmarks.update(|list| list.retain(|b| b.id != bid));
                    }
                },
            );
        }
    };

    // ── Load book view (branches on source) ──────────────────────────────────
    match source.clone() {
        ReaderSource::Beta(tok) => {
            let tok2 = tok.clone();
            api::get::<BetaReaderView>(&format!("/api/beta/{}", tok2), move |result| {
                match result {
                    Ok(data) => {
                        if let Some(ref fs) = data.font_settings {
                            fonts::load_book_fonts(fs);
                        }
                        bookmarks.set(data.bookmarks.clone());
                        let resume = data.last_chapter_id.clone();
                        let resume_page = data.last_page as i32;
                        view_data.set(Some(data));
                        // Resume where the reader left off, if permitted.
                        if let Some(cid) = resume {
                            open_chapter(cid, resume_page);
                        }
                    }
                    Err(e) => {
                        error_msg.set(Some(e.message));
                    }
                }
            });

            // Fetch feedback
            let tokf = tok.clone();
            api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tokf), move |result| {
                if let Ok(fb) = result {
                    feedback_list.set(fb);
                }
            });

            // Check session first, then auto-claim if logged in.
            // Auth check must come first — the claim endpoint's Session extractor
            // could create a new empty session that overwrites the valid cookie.
            {
                let tokc = tok.clone();
                let store = use_store::<AppStore>();
                if store.current_user.get().is_none() {
                    api::get::<plotweb_common::User>("/api/auth/me", move |result| {
                        if let Ok(user) = result {
                            store.current_user.set(Some(user));
                        }
                        if store.current_user.get().is_some() {
                            api::post::<_, serde_json::Value>(&format!("/api/beta/{}/claim", tokc), &serde_json::json!({}), move |_result| {});
                        }
                    });
                } else if store.current_user.get().is_some() {
                    api::post::<_, serde_json::Value>(&format!("/api/beta/{}/claim", tokc), &serde_json::json!({}), move |_result| {});
                }
            }

            // Connect WebSocket for real-time feedback
            {
                let ws_url = crate::ws::ws_url(&format!("/api/beta/{}/feedback/ws", tok));
                crate::ws::connect_feedback_ws(&ws_url, move |msg| {
                    match msg {
                        crate::ws::WsMessage::NewFeedback(fb) => {
                            feedback_list.update(|list| {
                                if !list.iter().any(|f| f.id == fb.id) {
                                    list.insert(0, fb);
                                }
                            });
                            // Mark the incoming quote in the prose if it belongs
                            // to the chapter currently on screen.
                            refresh_marks();
                        }
                        crate::ws::WsMessage::NewReply { feedback_id, reply } => {
                            feedback_list.update(|list| {
                                if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                                    if !fb.replies.iter().any(|r| r.id == reply.id) {
                                        fb.replies.push(reply);
                                    }
                                }
                            });
                        }
                        crate::ws::WsMessage::FeedbackResolved { feedback_id, resolved } => {
                            feedback_list.update(|list| {
                                if let Some(fb) = list.iter_mut().find(|f| f.id == feedback_id) {
                                    fb.resolved = resolved;
                                }
                            });
                        }
                        crate::ws::WsMessage::FeedbackDeleted { feedback_id } => {
                            feedback_list.update(|list| list.retain(|f| f.id != feedback_id));
                            // Its mark and any open popover pointing at it will
                            // be stale until the next chapter (re)load; at minimum
                            // don't leave the popover open on a deleted item.
                            if popover_feedback_id.get().as_deref() == Some(feedback_id.as_str()) {
                                popover_feedback_id.set(None);
                            }
                        }
                    }
                });
            }
        }
        ReaderSource::AuthorPreview(bid) => {
            // Build an equivalent in-memory view from the authenticated author
            // endpoints. No feedback / progress / bookmark writes in this mode.
            let bid_ch = bid.clone();
            api::get::<Book>(&format!("/api/books/{}", bid), move |book| {
                api::get::<Vec<Chapter>>(&format!("/api/books/{}/chapters", bid_ch), move |chapters| {
                match (book, chapters) {
                    (Ok(book), Ok(chs)) => {
                        if let Some(fs) = &book.font_settings {
                            fonts::load_book_fonts(fs);
                        }
                        let mut summaries: Vec<BetaChapterSummary> = chs
                            .iter()
                            .map(|c| BetaChapterSummary {
                                id: c.id.clone(),
                                title: c.title.clone(),
                                sort_order: c.sort_order,
                            })
                            .collect();
                        summaries.sort_by_key(|s| s.sort_order);
                        let view = BetaReaderView {
                            book_title: book.title.clone(),
                            book_description: book.description.clone(),
                            reader_name: "Preview".to_string(),
                            chapters: summaries,
                            font_settings: book.font_settings.clone(),
                            cover_image: book.cover_image.clone(),
                            last_chapter_id: None,
                            last_page: 0,
                            bookmarks: Vec::new(),
                        };
                        view_data.set(Some(view));
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        error_msg.set(Some(e.message));
                    }
                }
                });
            });
        }
    }

    // Sidebar chapter click → open at page 0.
    let load_chapter = move |chapter_id: String| {
        move || {
            open_chapter(chapter_id.clone(), 0);
        }
    };

    // Feedback text-selection is beta-only (no feedback panel in preview).
    if !is_preview {
    // Shared selection handler for both mouse and touch. Anchors the compose
    // tooltip on the *selection's own* bounding rect (not raw pointer coords)
    // so the flip-above math has a real box to reason about — the mockup's
    // popover flip needs the same. Falls back to the raw client coords when a
    // range rect isn't available (defensive; shouldn't happen with a live
    // selection).
    let handle_selection = std::rc::Rc::new(move |client_x: i32, client_y: i32| {
        let Some(doc) = crate::platform::document() else { return; };
        let sel = match doc.get_selection().ok().flatten() {
            Some(s) => s,
            None => return,
        };

        let text = sel.to_string().as_string().unwrap_or_default();
        if text.trim().is_empty() {
            tooltip_visible.set(false);
            return;
        }

        // Get context block (parent paragraph text) and the selection's rect.
        let mut context = String::new();
        let mut anchor_rect: Option<web_sys::DomRect> = None;
        if sel.range_count() > 0 {
            if let Ok(range) = sel.get_range_at(0) {
                anchor_rect = Some(range.get_bounding_client_rect());
                let container = range.common_ancestor_container().ok();
                let mut node = container;
                // Walk up to find block element
                while let Some(n) = node {
                    if let Ok(el) = n.clone().dyn_into::<web_sys::Element>() {
                        let tag = el.tag_name().to_lowercase();
                        if matches!(tag.as_str(), "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li") {
                            context = el.text_content().unwrap_or_default();
                            break;
                        }
                    }
                    node = n.parent_node();
                }
                if context.len() > 200 {
                    context.truncate(200);
                }
            }
        }

        tooltip_selected_text.set(text);
        tooltip_context.set(context);
        tooltip_comment.set(String::new());
        popover_feedback_id.set(None);

        let (left, top, bottom) = match &anchor_rect {
            Some(r) => (r.left(), r.top(), r.bottom()),
            None => (client_x as f64, client_y as f64, client_y as f64),
        };
        if let Some(frame) = doc.query_selector("#reader-viewport").ok().flatten() {
            let frame_rect = frame.get_bounding_client_rect();
            let (x, y, _flip) = compute_popover_position(
                left, top, bottom,
                frame_rect.top(), frame_rect.bottom(),
                160.0,
            );
            popover_x.set(x);
            popover_y.set(y);
        } else {
            popover_x.set(left);
            popover_y.set(bottom + 10.0);
        }
        tooltip_visible.set(true);
    });

    // Set up text selection listener for feedback tooltip (mouse)
    if let Some(document) = crate::platform::document() {
        let handle = handle_selection.clone();
        let mouseup_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
            let target = match event.target() {
                Some(t) => t,
                None => return,
            };
            let el: web_sys::Element = match target.dyn_into() {
                Ok(e) => e,
                Err(_) => return,
            };
            if el.closest("#reader-content").ok().flatten().is_none() {
                return;
            }
            handle(event.client_x(), event.client_y() - 10);
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("mouseup", mouseup_closure.as_ref().unchecked_ref())
            .ok();
        mouseup_closure.forget();
    }

    // Set up text selection listener for feedback tooltip (touch)
    if let Some(document) = crate::platform::document() {
        let handle = handle_selection.clone();
        let touchend_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::TouchEvent| {
            let target: web_sys::EventTarget = match event.target() {
                Some(t) => t,
                None => return,
            };
            let el: web_sys::Element = match target.dyn_into() {
                Ok(e) => e,
                Err(_) => return,
            };
            if el.closest("#reader-content").ok().flatten().is_none() {
                return;
            }
            // Delay to let mobile browser finalize selection
            let handle = handle.clone();
            let closure = wasm_bindgen::closure::Closure::once(move || {
                handle(0, 0);
            });
            if let Some(window) = crate::platform::window() {
                window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        closure.as_ref().unchecked_ref(),
                        100,
                    ).ok();
            }
            closure.forget();
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("touchend", touchend_closure.as_ref().unchecked_ref())
            .ok();
        touchend_closure.forget();
    }
    } // end if !is_preview (feedback selection)

    // ── Keyboard paging (Arrow left/right), ignoring text inputs ─────────────
    // Native: inert — this hangs off a document-level `keydown` listener. Porting
    // it needs rinch's key handling, and with pagination itself inert on desktop
    // there are no pages to flip between yet.
    crate::web_only! {
        let keydown_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            if let Some(t) = event.target() {
                if let Ok(el) = t.dyn_into::<web_sys::Element>() {
                    let tag = el.tag_name().to_lowercase();
                    if tag == "textarea" || tag == "input" {
                        return;
                    }
                }
            }
            match event.key().as_str() {
                "ArrowLeft" => go_to_page(current_page.get() - 1),
                "ArrowRight" => go_to_page(current_page.get() + 1),
                _ => {}
            }
        }) as Box<dyn FnMut(_)>);
        if let Some(document) = crate::platform::document() {
            document
                .add_event_listener_with_callback("keydown", keydown_closure.as_ref().unchecked_ref())
                .ok();
        }
        keydown_closure.forget();
    }

    // ── Touch swipe paging (skipped while text is selected for feedback) ─────
    // Native: inert — document-level touch listeners, and the desktop shell has
    // no touch input.
    crate::web_only! {
        let swipe_start = std::rc::Rc::new(std::cell::Cell::new(0.0f64));
        {
            let s = swipe_start.clone();
            let touchstart_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::TouchEvent| {
                if let Some(t) = event.touches().get(0) {
                    s.set(t.client_x() as f64);
                }
            }) as Box<dyn FnMut(_)>);
            if let Some(document) = crate::platform::document() {
                document
                    .add_event_listener_with_callback("touchstart", touchstart_closure.as_ref().unchecked_ref())
                    .ok();
            }
            touchstart_closure.forget();
        }
        {
            let s = swipe_start.clone();
            let touchend_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::TouchEvent| {
                let within = event
                    .target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| el.closest("#reader-viewport").ok().flatten())
                    .is_some();
                if !within || !selection_is_empty() {
                    return;
                }
                let end_x = event.changed_touches().get(0).map(|t| t.client_x() as f64).unwrap_or(s.get());
                let dx = end_x - s.get();
                if dx.abs() > 45.0 {
                    if dx < 0.0 {
                        go_to_page(current_page.get() + 1);
                    } else {
                        go_to_page(current_page.get() - 1);
                    }
                }
            }) as Box<dyn FnMut(_)>);
            if let Some(document) = crate::platform::document() {
                document
                    .add_event_listener_with_callback("touchend", touchend_closure.as_ref().unchecked_ref())
                    .ok();
            }
            touchend_closure.forget();
        }
    }

    // ── Re-flow pages on window resize ───────────────────────────────────────
    // Native: inert — there is nothing to re-flow while pagination is web-only.
    crate::web_only! {
        let resize_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
            repaginate(current_page.get());
        }) as Box<dyn FnMut(_)>);
        if let Some(window) = crate::platform::window() {
            window
                .add_event_listener_with_callback("resize", resize_closure.as_ref().unchecked_ref())
                .ok();
        }
        resize_closure.forget();
    }

    // Submit feedback
    let submit_feedback = move || {
        let comment = tooltip_comment.get();
        if comment.trim().is_empty() {
            return;
        }
        let chapter_id = match active_chapter_id.get() {
            Some(id) => id,
            None => return,
        };
        let selected_text = tooltip_selected_text.get();
        let context_block = tooltip_context.get();
        let tok = token_signal.get();

        tooltip_visible.set(false);
        // Clear the draft now that it has been consumed — the textarea is bound
        // to this signal, so this is what empties it visually.
        tooltip_comment.set(String::new());

        let req = CreateBetaFeedbackRequest {
            chapter_id,
            selected_text,
            context_block,
            comment,
        };
        let tok_refresh = tok.clone();
        api::post::<_, serde_json::Value>(&format!("/api/beta/{}/feedback", tok), &req, move |result| {
            if result.is_ok() {
                // Refresh feedback list
                api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tok_refresh), move |fb_result| {
                    if let Ok(fb) = fb_result {
                        feedback_list.set(fb);
                        // Mark the quote just submitted, right where it was
                        // selected — no need to wait for a chapter reload.
                        refresh_marks();
                    }
                });
            }
        });
    };

    // Reply to feedback
    let reply_to_feedback = move |feedback_id: String| {
        move || {
            let content = reply_drafts
                .get()
                .get(&feedback_id)
                .cloned()
                .unwrap_or_default();
            if content.trim().is_empty() {
                return;
            }
            let clear_key = feedback_id.clone();
            reply_drafts.update(|m| { m.remove(&clear_key); });
            let tok = token_signal.get();
            let fid = feedback_id.clone();
            let req = CreateBetaReplyRequest { content };
            let tok_refresh = tok.clone();
            api::post::<_, serde_json::Value>(
                &format!("/api/beta/{}/feedback/{}/replies", tok, fid),
                &req,
                move |result| {
                    if result.is_ok() {
                        // Refresh
                        api::get::<Vec<BetaFeedback>>(&format!("/api/beta/{}/feedback", tok_refresh), move |fb_result| {
                            if let Ok(fb) = fb_result {
                                feedback_list.set(fb);
                            }
                        });
                    }
                },
            );
        }
    };

    let toggle_contents = move || {
        contents_open.update(|v| *v = !*v);
    };

    rsx! {
        Fragment {
            style { {READER_CSS} }
            style { {editor_utils::EDITOR_CSS} }

            // Font styles from book settings
            style {
                {move || {
                    let data = view_data.get();
                    let fs = data.as_ref()
                        .and_then(|d| d.font_settings.as_ref())
                        .cloned()
                        .unwrap_or_default();

                    let h1 = fs.h1.as_deref().unwrap_or("Macondo Swash Caps");
                    let body = fs.body.as_deref().unwrap_or("Playwrite DE Grund");
                    let quote = fs.quote.as_deref().unwrap_or("inherit");
                    let p_spacing = fs.paragraph_spacing.unwrap_or(8.0);
                    let p_indent = fs.paragraph_indent.unwrap_or(0.0);

                    format!(
                        ".reader-workspace {{ --rinch-font-family: '{body}', serif; font-family: '{body}', serif; }}
                         .reader-contents-title {{ font-family: '{h1}', cursive; }}
                         .reader-topbar-book {{ font-family: '{h1}', cursive; }}
                         .reader-mobile-topbar {{ font-family: '{body}', serif; }}
                         .reader-mobile-topbar > .rinch-text {{ font-family: '{h1}', cursive; }}
                         .feedback-tooltip {{ font-family: '{body}', serif; }}
                         .feedback-tooltip .rinch-button {{ font-family: '{body}', serif; }}
                         .feedback-tooltip-quote {{ font-family: '{body}', serif; }}
                         .reader-content {{ font-family: '{body}', serif; }}
                         .reader-content p {{ margin: 0 0 {p_spacing}px 0; text-indent: {p_indent}px; }}
                         .reader-welcome h2 {{ font-family: '{h1}', cursive; }}
                         .reader-content h1, .reader-content h2,
                         .reader-content h3 {{ font-family: '{h1}', cursive; }}
                         .reader-content blockquote {{ font-family: '{quote}', serif; }}"
                    )
                }}
            }

            if error_msg.get().is_some() {
                div { class: "reader-error",
                    div {
                        h2 { "PlotWeb" }
                        Text { color: "dimmed", size: "lg",
                            {move || error_msg.get().unwrap_or_default()}
                        }
                    }
                }
            } else if view_data.get().is_none() {
                div { class: "reader-welcome",
                    Text { color: "dimmed", "Loading..." }
                }
            } else {
                div { class: "reader-workspace",
                    // Mobile topbar
                    div { class: "reader-mobile-topbar",
                        ActionIcon {
                            variant: "subtle",
                            size: "sm",
                            onclick: toggle_contents,
                            {render_tabler_icon(__scope, TablerIcon::Menu2, TablerIconStyle::Outline)}
                        }
                        Text { weight: "600", size: "sm",
                            {move || current_chapter.get().map(|c| c.title.clone()).unwrap_or_else(|| "Select a chapter".into())}
                        }
                        div {
                            style: "display: flex; align-items: center; gap: 4px;",
                            if !is_preview {
                                ActionIcon {
                                    variant: "subtle",
                                    size: "sm",
                                    onclick: add_bookmark,
                                    {render_tabler_icon(__scope, TablerIcon::Bookmark, TablerIconStyle::Outline)}
                                }
                            }
                        }
                    }

                    // Desktop header — quiet metadata, not a toolbar. Contents
                    // opens from here; the feedback rail is gone (marks in the
                    // prose + the compose tooltip cover that job now).
                    div { class: "reader-topbar",
                        ActionIcon {
                            variant: "subtle",
                            size: "sm",
                            onclick: toggle_contents,
                            {render_tabler_icon(__scope, TablerIcon::Menu2, TablerIconStyle::Outline)}
                        }
                        span { class: "reader-topbar-book",
                            {move || view_data.get().map(|d| d.book_title.clone()).unwrap_or_default()}
                        }
                        span { "\u{b7}" }
                        span {
                            {move || current_chapter.get().map(|c| c.title.clone()).unwrap_or_else(|| "Select a chapter".into())}
                        }
                        div { class: "reader-topbar-sp" }
                        if is_preview {
                            Badge { variant: "light", size: "sm", "Preview" }
                        } else {
                            span {
                                "Reading as "
                                strong { {move || view_data.get().map(|d| d.reader_name.clone()).unwrap_or_default()} }
                            }
                            ActionIcon {
                                variant: "subtle",
                                size: "sm",
                                onclick: add_bookmark,
                                {render_tabler_icon(__scope, TablerIcon::Bookmark, TablerIconStyle::Outline)}
                            }
                        }
                    }

                    if current_chapter.get().is_none() {
                        div { class: "reader-welcome",
                            div {
                                h2 { "Welcome" }
                                Text { color: "dimmed",
                                    "Open Contents to start reading."
                                }
                            }
                        }
                    } else {
                        div { class: "reader-reading-col",
                            div { class: "reader-viewport", id: "reader-viewport",
                                // Margin page-turn hit targets — nearly invisible
                                // until hovered; the keyboard/swipe gestures are
                                // the primary way to turn a page.
                                div {
                                    class: {move || if current_page.get() <= 0 && adjacent_chapter_id(view_data, active_chapter_id, -1).is_none() { "reader-turn l disabled" } else { "reader-turn l" }},
                                    onclick: move || go_to_page(current_page.get() - 1),
                                    "\u{2039}"
                                }
                                div { class: "reader-page-frame",
                                    {reader_content_node(__scope, content_el)}
                                }
                                div {
                                    class: {move || if current_page.get() >= total_pages.get() - 1 && adjacent_chapter_id(view_data, active_chapter_id, 1).is_none() { "reader-turn r disabled" } else { "reader-turn r" }},
                                    onclick: move || go_to_page(current_page.get() + 1),
                                    "\u{203a}"
                                }
                            }

                            // Folio: a 2px progress line, then chapter / page /
                            // position-in-book — replaces the old bordered page
                            // bar (two chevrons + "1 / 1").
                            div { class: "reader-folio",
                                div { class: "reader-folio-progress",
                                    i { style: {move || format!("width: {}%", if total_pages.get() > 1 { (current_page.get() + 1) as f64 / total_pages.get() as f64 * 100.0 } else { 100.0 })} }
                                }
                                div { class: "reader-folio-row",
                                    span {
                                        {move || current_chapter.get().map(|c| c.title.clone()).unwrap_or_default()}
                                    }
                                    div { class: "reader-topbar-sp" }
                                    span {
                                        {move || format!("{} / {}", current_page.get() + 1, total_pages.get())}
                                    }
                                    div { class: "reader-topbar-sp" }
                                    span {
                                        {move || {
                                            let data = view_data.get();
                                            let total = data.as_ref().map(|d| d.chapters.len()).unwrap_or(0);
                                            let idx = data.as_ref()
                                                .zip(active_chapter_id.get())
                                                .and_then(|(d, cid)| d.chapters.iter().position(|c| c.id == cid))
                                                .map(|i| i + 1)
                                                .unwrap_or(0);
                                            format!("CH {} OF {}", idx, total)
                                        }}
                                    }
                                }
                            }
                        }
                    }

                    // ── Contents panel — on demand, opened from the header ────
                    div {
                        class: {move || if contents_open.get() { "reader-contents-backdrop open" } else { "reader-contents-backdrop" }},
                        onclick: move || contents_open.set(false),
                    }
                    div {
                        class: {move || if contents_open.get() { "reader-contents open" } else { "reader-contents" }},
                        img {
                            class: "reader-contents-cover",
                            style: {move || if view_data.get().and_then(|d| d.cover_image.clone()).is_some() { String::new() } else { "display:none;".to_string() }},
                            src: {move || view_data.get().and_then(|d| d.cover_image.clone()).unwrap_or_default()},
                            alt: "Book cover",
                        }
                        div { class: "reader-contents-title",
                            {move || view_data.get().map(|d| d.book_title.clone()).unwrap_or_default()}
                        }
                        div { class: "reader-contents-meta",
                            "Reading as "
                            strong { {move || view_data.get().map(|d| d.reader_name.clone()).unwrap_or_default()} }
                        }
                        div { class: "reader-contents-header",
                            "Contents"
                            span {
                                {move || format!("{}", view_data.get().map(|d| d.chapters.len()).unwrap_or(0))}
                            }
                        }
                        div { class: "reader-contents-chapters",
                            for ch in view_data.get().map(|d| d.chapters.clone()).unwrap_or_default() {
                                {reader_chapter_item(__scope, ch.id.clone(), ch.title.clone(), ch.sort_order, active_chapter_id, load_chapter)}
                            }
                        }
                        if !is_preview && (!bookmarks.get().is_empty() || !feedback_list.get().is_empty()) {
                            div { class: "reader-contents-footer",
                                if !bookmarks.get().is_empty() {
                                    div { class: "reader-contents-header", "Bookmarks" }
                                    for bm in bookmarks.get() {
                                        {reader_bookmark_item(__scope, bm, open_chapter, delete_bookmark)}
                                    }
                                }
                                if !feedback_list.get().is_empty() {
                                    div { class: "reader-contents-header",
                                        "Your notes"
                                        span { {move || format!("{}", feedback_list.get().len())} }
                                    }
                                    for fb in feedback_list.get() {
                                        {reader_note_item(__scope, fb, contents_open, popover_feedback_id, open_chapter)}
                                    }
                                }
                            }
                        }
                        if store.current_user.get().is_some() {
                            div { class: "reader-contents-back",
                                Button {
                                    variant: "subtle",
                                    size: "xs",
                                    onclick: move || router::navigate(Route::Dashboard),
                                    "\u{2190} Dashboard"
                                }
                            }
                        }
                    }
                }
            }

            // Compose tooltip (new feedback on a fresh selection, beta only)
            // and the popover for an existing item (click its `.rd-mark`) share
            // the same floating box and CSS — only one is ever open at a time.
            if !is_preview {
                {reader_feedback_floating_box(
                    __scope,
                    tooltip_visible, tooltip_comment,
                    popover_feedback_id, popover_x, popover_y,
                    feedback_list, reply_drafts, reply_to_feedback,
                    submit_feedback,
                )}
            } // end if !is_preview (feedback tooltip / popover)
        }
    }
}

/// The one floating box that does either job — composing new feedback on a
/// fresh selection, or showing an existing item's popover — never both at
/// once. Factored out of `reader_body` mainly so the nested `if let Some(fid)
/// = ... { if let Some(fb) = ... { ... } }` lookup (an existing feedback item,
/// found by id in the list) lives in one place: rinch's `rsx!` re-derives an
/// `if let`'s binding inside its own generated closure rather than reusing
/// the token as written, so rustc's unused-variable lint fires on `fid`/`fb`
/// here even though the macro's body closure genuinely uses them — scoping
/// the `#[allow]` to this small function keeps it from hiding a real unused-
/// variable bug anywhere else in the much larger `reader_body`.
#[allow(unused_variables)]
fn reader_feedback_floating_box<S>(
    __scope: &mut RenderScope,
    tooltip_visible: Signal<bool>,
    tooltip_comment: Signal<String>,
    popover_feedback_id: Signal<Option<String>>,
    popover_x: Signal<f64>,
    popover_y: Signal<f64>,
    feedback_list: Signal<Vec<BetaFeedback>>,
    reply_drafts: Signal<std::collections::HashMap<String, String>>,
    reply_to_feedback: impl Fn(String) -> S + 'static + Copy,
    submit_feedback: impl Fn() + 'static + Copy,
) -> NodeHandle
where
    S: Fn() + 'static,
{
    rsx! {
        div {
            class: {move || if tooltip_visible.get() || popover_feedback_id.get().is_some() { "feedback-tooltip visible" } else { "feedback-tooltip" }},
            style: {move || format!("left: {}px; top: {}px;", popover_x.get(), popover_y.get())},
            if let Some(fid) = popover_feedback_id.get() {
                if let Some(fb) = feedback_list.get().into_iter().find(|f| f.id == fid) {
                    {reader_feedback_popover(__scope, fb, reply_drafts, reply_to_feedback)}
                }
                div { class: "feedback-tooltip-actions",
                    Button {
                        variant: "subtle",
                        size: "xs",
                        onclick: move || popover_feedback_id.set(None),
                        "Close"
                    }
                }
            } else {
                textarea {
                    placeholder: "Leave your feedback...",
                    id: "feedback-tooltip-textarea",
                    value: {move || tooltip_comment.get()},
                    oninput: move |v: String| tooltip_comment.set(v),
                }
                div { class: "feedback-tooltip-actions",
                    Button {
                        variant: "subtle",
                        size: "xs",
                        onclick: move || tooltip_visible.set(false),
                        "Cancel"
                    }
                    Button {
                        size: "xs",
                        onclick: move || submit_feedback(),
                        "Submit"
                    }
                }
            }
        }
    }
}
