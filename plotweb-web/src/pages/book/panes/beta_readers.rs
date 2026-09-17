//! Beta Readers pane: link cards and the all-feedback overview.

use rinch::prelude::*;
use rinch_tabler_icons::{TablerIcon, TablerIconStyle, render_tabler_icon};
use plotweb_common::BetaReaderLink;

use crate::store::AppStore;

use super::super::feedback::overview_feedback_item;
use super::super::state::BookState;
use super::super::BookPane;

fn render_beta_link_card<CB, TG, DL, CBO, TGO, DLO>(
    __scope: &mut RenderScope,
    link: BetaReaderLink,
    edit_beta_reader_name: Signal<String>,
    edit_beta_max_chapter: Signal<Option<i64>>,
    edit_beta_max_chapter_text: Signal<String>,
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
                            // Seed the bound input text from the link being edited
                            // (stored 0-indexed, displayed 1-indexed).
                            edit_beta_max_chapter_text.set(
                                edit_link.max_chapter_index.map(|v| (v + 1).to_string()).unwrap_or_default(),
                            );
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

/// Render the Beta Readers pane (CSS toggle).
#[allow(clippy::too_many_arguments)]
pub(in crate::pages::book) fn render<CB, TG, DL, AR, RF, DF, NF, CBO, TGO, DLO, ARO, RFO, DFO, NFO>(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
    copy_beta_link: CB,
    toggle_beta_link_active: TG,
    delete_beta_link: DL,
    author_reply: AR,
    resolve_feedback: RF,
    delete_feedback: DF,
    navigate_to_feedback: NF,
) -> NodeHandle
where
    CB: Fn(String) -> CBO + 'static + Copy,
    TG: Fn(String, bool) -> TGO + 'static + Copy,
    DL: Fn(String) -> DLO + 'static + Copy,
    AR: Fn(String) -> ARO + 'static + Copy,
    RF: Fn(String) -> RFO + 'static + Copy,
    DF: Fn(String) -> DFO + 'static + Copy,
    NF: Fn(String, String, String) -> NFO + 'static + Copy,
    CBO: Fn() + 'static,
    TGO: Fn() + 'static,
    DLO: Fn() + 'static,
    ARO: Fn() + 'static,
    RFO: Fn() + 'static,
    DFO: Fn() + 'static,
    NFO: Fn() + 'static,
{
    let BookState {
        active_pane,
        new_beta_reader_name,
        new_beta_max_chapter,
        new_beta_max_chapter_text,
        new_beta_pin_version,
        new_beta_username,
        beta_link_error,
        show_beta_link_modal,
        beta_links,
        beta_feedback,
        edit_beta_reader_name,
        edit_beta_max_chapter,
        edit_beta_max_chapter_text,
        edit_beta_pinned,
        edit_beta_username,
        editing_beta_link,
        reply_drafts,
        ..
    } = state;
    rsx! {
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
                            new_beta_max_chapter_text.set(String::new());
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
                            edit_beta_max_chapter_text,
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
                            {overview_feedback_item(__scope, fb.0, fb.1, reply_drafts, author_reply, resolve_feedback, delete_feedback, navigate_to_feedback)}
                        }
                    }
                }
            }
        }
    }
}
