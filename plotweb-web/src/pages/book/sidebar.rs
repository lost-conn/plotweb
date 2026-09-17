//! The book sidebar's chapter list item. The rest of the sidebar (title, nav
//! headers, footer) is small enough to stay inline in `mod.rs`'s top-level layout.

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};

use super::BookPane;

/// Render a single sidebar chapter item. Extracted to avoid rsx for-loop move issues.
pub(super) fn sidebar_chapter_item<O, M, FO, FM>(
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
