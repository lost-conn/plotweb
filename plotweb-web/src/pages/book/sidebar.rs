//! The book sidebar's chapter list item. The rest of the sidebar (title, nav
//! headers, footer) is small enough to stay inline in `mod.rs`'s top-level layout.

use rinch::prelude::*;

use super::panes::chapters::abbreviate_word_count;
use super::BookPane;

/// Render a single sidebar chapter item: title plus its word count
/// (`.ch`/`.nm`/`.wc` in the mockup). No reorder affordance here — dragging
/// lives in the chapters pane rows (`panes/chapters.rs`), which is where the
/// full list with headroom for a grip handle actually is; the sidebar nav is
/// too narrow for a comfortable drag target.
pub(super) fn sidebar_chapter_item<O, FO>(
    __scope: &mut RenderScope,
    id: String,
    title: String,
    word_count: u64,
    active_pane: Signal<BookPane>,
    open_chapter: O,
) -> NodeHandle
where
    O: Fn(String) -> FO + 'static + Copy,
    FO: Fn() + 'static,
{
    let cid = id.clone();
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
            onclick: open_chapter(cid),

            span { class: "sidebar-chapter-name", {title} }
            span { class: "sidebar-chapter-wc", {abbreviate_word_count(word_count)} }
        }
    }
}
