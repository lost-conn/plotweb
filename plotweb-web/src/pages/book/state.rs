//! `BookState` — the book page's signals, grouped by concern.
//!
//! `book_page` used to declare ~89 `Signal::new` locals directly in its body and
//! thread the ones each helper needed through long parameter lists (`render_note_card`
//! alone took 20). Rinch signals are `Copy`, so bundling them into one `Clone, Copy`
//! struct (mirroring `AppStore` in `store.rs`) lets every pane/modal/helper function
//! just take `state: BookState` instead. This file only relocates the declarations —
//! every comment below carries the same reasoning it did inline in `book_page`, moved
//! with the field it documents.

use std::collections::{HashMap, HashSet};

use plotweb_common::{
    BetaFeedback, BetaReaderLink, Chapter, CommitDiff, CommitInfo, FontSettings,
    ImportChapter, ImportPreviewChapter,
};
use rinch_core::Signal;

use crate::rinch_backend::EditorHandle;

use super::BookPane;

#[derive(Clone, Copy)]
pub(super) struct BookState {
    // ── Book / chapter editor state ─────────────────────────────
    pub active_pane: Signal<BookPane>,
    pub chapters_collapsed: Signal<bool>,
    pub chapter_title: Signal<String>,
    pub save_status: Signal<&'static str>,
    /// The author-facing half of a save that didn't fully land: the status indicator is
    /// four words in a header, which is not enough to notice that writing is not being
    /// stored. Set from the server's receipt; cleared by the next clean save.
    pub save_alert: Signal<Option<String>>,
    pub editor_word_count: Signal<u64>,
    /// The chapter id whose content is currently loaded in the editor model. `None`
    /// during the fetch window of a chapter switch — save-on-leave / autosave only
    /// persist when this matches the target chapter, so a not-yet-loaded editor
    /// (still holding the previous chapter) can't overwrite the wrong chapter.
    pub loaded_chapter_id: Signal<Option<String>>,
    /// Has this surface been edited since the document was loaded?
    ///
    /// Save-on-leave used to fire unconditionally, so merely opening a chapter or note
    /// and navigating away rewrote it with whatever the editor happened to hold. That is
    /// harmless while the editor holds what is stored, and destroys the stored copy the
    /// moment it doesn't — a divergent canonical copy, a failed load, the chapter
    /// crosstalk bug. A note lost a paragraph in production exactly this way: it was
    /// opened, showed the (blank) canonical copy of a diverged document, and the walk
    /// away wrote that blank over git.
    ///
    /// Set where a save is *requested* (every path that means "something changed" goes
    /// through the schedulers), cleared when a document loads and after a save lands.
    pub chapter_dirty: Signal<bool>,
    pub note_dirty: Signal<bool>,
    pub auto_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,

    /// True while the author is actively typing in the chapter editor — sidebar,
    /// editor header, footer and feedback rail fade (opacity + `pointer-events:
    /// none`, never width/margin, so the prose column never shifts) while this is
    /// set. Set on `EditorHandle::on_change` (a real content edit — cross-platform,
    /// unlike guessing at DOM `keydown` on a surface that isn't `contenteditable`);
    /// cleared on pointer move, Escape, or a short idle timeout.
    pub editor_writing: Signal<bool>,
    /// Idle-return timer for `editor_writing` — reset on every edit, fires to
    /// bring the chrome back after a pause in typing.
    pub editor_writing_idle_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,

    /// Model-first prose editors (rinch-editor-view), one per prose surface. Stored in
    /// Signals so the (Copy) save/switch closures can grab a clone via `.get()`.
    pub chapter_handle: Signal<EditorHandle>,
    pub note_handle: Signal<EditorHandle>,

    pub bid_signal: Signal<String>,

    // ── Chapter inline rename / add (Tier 1 — no overlay) ────────
    /// The chapter row currently in its inline rename state (title becomes a
    /// text field in place), or `None` when no row is being renamed.
    pub editing_chapter_id: Signal<Option<String>>,
    /// Draft text for whichever row is being edited — the rename row above, or
    /// the empty draft row appended by "Add chapter" (see `pending_new_chapter`).
    pub editing_chapter_title: Signal<String>,
    /// Set the instant "Add chapter" is clicked: the chapters pane appends one
    /// empty row and focuses its title field instead of opening a dialog to ask
    /// for a title. Cleared once that row commits (Enter/blur) or is cancelled
    /// (Escape, which discards the still-uncreated chapter entirely).
    pub pending_new_chapter: Signal<bool>,
    /// Inline chapter title save timer
    pub chapter_title_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    /// The chapter targeted by the Tier-2 delete confirm dialog — (id, title,
    /// word count), or `None` when it's closed. Mirrors the dashboard's book
    /// delete confirm (`pages/dashboard.rs`'s `delete_target`).
    pub delete_chapter_target: Signal<Option<(String, String, u64)>>,

    // ── Chapters pane: drag-to-reorder + the `⋯` overflow menu ──────
    /// The chapter id currently being dragged, `None` when no drag is active.
    /// Mirrors `dragging_note_id` (see `panes/notes.rs`) — same native-drag
    /// approach, applied to a flat list instead of a tree.
    pub dragging_chapter_id: Signal<Option<String>>,
    /// Row index the dragged chapter would land on if dropped now.
    pub chapter_drop_target: Signal<Option<usize>>,
    /// Whether the chapters pane header's `⋯` (Import/Export) menu is open.
    pub show_chapters_menu: Signal<bool>,

    // ── Typography state ─────────────────────────────────────────
    pub font_settings: Signal<FontSettings>,
    /// Debounce handle for font-settings saves, written by the reactive Typography
    /// picker (cross-platform — see `font_picker` / `save_font_settings`).
    pub font_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    /// Which font slot's dropdown is currently open ("" = none). Shared across the
    /// six pickers so only one is open at a time.
    pub open_slot: Signal<&'static str>,

    // ── Beta reader state ────────────────────────────────────────
    pub beta_links: Signal<Vec<BetaReaderLink>>,
    pub beta_feedback: Signal<Vec<BetaFeedback>>,
    pub show_beta_link_modal: Signal<bool>,
    pub new_beta_reader_name: Signal<String>,
    pub new_beta_max_chapter: Signal<Option<i64>>,
    pub show_feedback_sidebar: Signal<bool>,
    pub _beta_reply_text: Signal<String>,

    /// Per-feedback author reply drafts, keyed by feedback id. The reply
    /// textareas are controlled off this map (`value:` + `oninput:`) so
    /// submitting reads state, not the DOM (the DOM read no-oped on native).
    pub reply_drafts: Signal<HashMap<String, String>>,

    /// Pending feedback scroll target: (selected_text, context_block)
    pub pending_feedback_scroll: Signal<Option<(String, String)>>,

    // Edit beta link signals
    pub editing_beta_link: Signal<Option<BetaReaderLink>>,
    pub edit_beta_reader_name: Signal<String>,
    pub edit_beta_max_chapter: Signal<Option<i64>>,
    /// Raw text of the "Max chapters" number inputs. Bound to the inputs with
    /// `value:` + `oninput:` and parsed on submit, so the value comes from state
    /// rather than a DOM read (which no-oped on native).
    pub new_beta_max_chapter_text: Signal<String>,
    pub edit_beta_max_chapter_text: Signal<String>,
    pub new_beta_pin_version: Signal<bool>,
    pub edit_beta_pinned: Signal<bool>,
    pub new_beta_username: Signal<String>,
    pub edit_beta_username: Signal<String>,
    pub beta_link_error: Signal<Option<String>>,

    // ── Import/export state ──────────────────────────────────────
    pub show_import_modal: Signal<bool>,
    pub import_preview: Signal<Vec<ImportPreviewChapter>>,
    pub import_filename: Signal<String>,
    pub import_loading: Signal<bool>,
    pub import_error: Signal<Option<String>>,
    pub import_file: Signal<Option<web_sys::File>>,
    /// Full chapter content stored separately (preview only has truncated text)
    pub import_full_chapters: Signal<Vec<ImportChapter>>,

    pub show_export_modal: Signal<bool>,
    pub export_format: Signal<&'static str>,
    pub export_selected: Signal<HashSet<String>>,
    pub export_loading: Signal<bool>,
    pub export_error: Signal<Option<String>>,

    // ── Notes state ───────────────────────────────────────────────
    pub show_note_modal: Signal<bool>,
    pub new_note_title: Signal<String>,
    pub new_note_parent_id: Signal<Option<String>>,
    pub new_note_color: Signal<String>,
    pub note_save_status: Signal<&'static str>,
    pub note_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    pub note_editor_title: Signal<String>,
    pub note_editor_color: Signal<Option<String>>,
    pub dragging_note_id: Signal<Option<String>>,
    pub drop_target: Signal<Option<(Option<String>, usize)>>,
    /// Floating drag ghost, positioned from ondragmove (see render_note_card).
    pub ghost_visible: Signal<bool>,
    pub ghost_pos: Signal<(f32, f32)>,
    pub ghost_label: Signal<String>,
    pub ghost_color: Signal<String>,

    // ── History state ─────────────────────────────────────────────
    pub history_commits: Signal<Vec<CommitInfo>>,
    pub history_preview_commit: Signal<Option<String>>,
    pub history_preview_chapters: Signal<Vec<Chapter>>,
    pub history_preview_content: Signal<Option<Chapter>>,
    pub history_diff: Signal<Option<CommitDiff>>,
    pub show_restore_confirm: Signal<Option<String>>,

    // ── Book settings modal ──────────────────────────────────────
    pub show_book_settings_modal: Signal<bool>,
    pub edit_book_title: Signal<String>,
    pub edit_book_desc: Signal<String>,
    pub edit_book_cover: Signal<Option<String>>,
}

impl BookState {
    pub fn new(book_id: &str) -> Self {
        Self {
            active_pane: Signal::new(BookPane::Chapters),
            chapters_collapsed: Signal::new(false),
            chapter_title: Signal::new(String::new()),
            save_status: Signal::new("saved"),
            save_alert: Signal::new(None),
            editor_word_count: Signal::new(0),
            loaded_chapter_id: Signal::new(None),
            chapter_dirty: Signal::new(false),
            note_dirty: Signal::new(false),
            auto_save_timer_id: Signal::new(None),

            editor_writing: Signal::new(false),
            editor_writing_idle_timer_id: Signal::new(None),

            chapter_handle: Signal::new(crate::rinch_backend::create_editor()),
            note_handle: Signal::new(crate::rinch_backend::create_editor()),

            bid_signal: Signal::new(book_id.to_string()),

            editing_chapter_id: Signal::new(None),
            editing_chapter_title: Signal::new(String::new()),
            pending_new_chapter: Signal::new(false),
            chapter_title_save_timer_id: Signal::new(None),
            delete_chapter_target: Signal::new(None),

            dragging_chapter_id: Signal::new(None),
            chapter_drop_target: Signal::new(None),
            show_chapters_menu: Signal::new(false),

            font_settings: Signal::new(FontSettings::default()),
            font_save_timer_id: Signal::new(None),
            open_slot: Signal::new(""),

            beta_links: Signal::new(Vec::new()),
            beta_feedback: Signal::new(Vec::new()),
            show_beta_link_modal: Signal::new(false),
            new_beta_reader_name: Signal::new(String::new()),
            new_beta_max_chapter: Signal::new(None),
            show_feedback_sidebar: Signal::new(false),
            _beta_reply_text: Signal::new(String::new()),

            reply_drafts: Signal::new(HashMap::new()),

            pending_feedback_scroll: Signal::new(None),

            editing_beta_link: Signal::new(None),
            edit_beta_reader_name: Signal::new(String::new()),
            edit_beta_max_chapter: Signal::new(None),
            new_beta_max_chapter_text: Signal::new(String::new()),
            edit_beta_max_chapter_text: Signal::new(String::new()),
            new_beta_pin_version: Signal::new(false),
            edit_beta_pinned: Signal::new(false),
            new_beta_username: Signal::new(String::new()),
            edit_beta_username: Signal::new(String::new()),
            beta_link_error: Signal::new(None),

            show_import_modal: Signal::new(false),
            import_preview: Signal::new(Vec::new()),
            import_filename: Signal::new(String::new()),
            import_loading: Signal::new(false),
            import_error: Signal::new(None),
            import_file: Signal::new(None),
            import_full_chapters: Signal::new(Vec::new()),

            show_export_modal: Signal::new(false),
            export_format: Signal::new("md"),
            export_selected: Signal::new(HashSet::new()),
            export_loading: Signal::new(false),
            export_error: Signal::new(None),

            show_note_modal: Signal::new(false),
            new_note_title: Signal::new(String::new()),
            new_note_parent_id: Signal::new(None),
            new_note_color: Signal::new("teal".to_string()),
            note_save_status: Signal::new("saved"),
            note_save_timer_id: Signal::new(None),
            note_editor_title: Signal::new(String::new()),
            note_editor_color: Signal::new(None),
            dragging_note_id: Signal::new(None),
            drop_target: Signal::new(None),
            ghost_visible: Signal::new(false),
            ghost_pos: Signal::new((0.0, 0.0)),
            ghost_label: Signal::new(String::new()),
            ghost_color: Signal::new("teal".to_string()),

            history_commits: Signal::new(Vec::new()),
            history_preview_commit: Signal::new(None),
            history_preview_chapters: Signal::new(Vec::new()),
            history_preview_content: Signal::new(None),
            history_diff: Signal::new(None),
            show_restore_confirm: Signal::new(None),

            show_book_settings_modal: Signal::new(false),
            edit_book_title: Signal::new(String::new()),
            edit_book_desc: Signal::new(String::new()),
            edit_book_cover: Signal::new(None),
        }
    }
}
