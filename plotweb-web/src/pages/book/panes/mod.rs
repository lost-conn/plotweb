//! The book workspace's eight panes. All eight are permanently mounted
//! siblings (see `mod.rs`'s top-level layout) toggled by `display:none` rather
//! than a `match` — this preserves the chapter/note editors' undo history
//! across pane switches. Each module here renders one pane's subtree; none of
//! them own the mount/unmount decision.

pub(super) mod beta_readers;
pub(super) mod calendar;
pub(super) mod chapters;
pub(super) mod editor;
pub(super) mod history;
pub(super) mod note_editor;
pub(super) mod notes;
pub(super) mod timeline;
pub(super) mod timeline_spine;
pub(super) mod typography;
