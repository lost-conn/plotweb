//! Typography pane: font pickers, spacing/indent selects, and the live preview.

use rinch::prelude::*;
use plotweb_common::FontSettings;

use crate::api;
use crate::fonts;
use crate::store::AppStore;

use super::super::BookPane;
use super::super::state::BookState;

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

/// Current family stored for a font slot, or "" if unset (the default shows via placeholder).
fn slot_family(fs: &FontSettings, slot: &str) -> String {
    match slot {
        "h1" => fs.h1.clone(),
        "h2" => fs.h2.clone(),
        "h3" => fs.h3.clone(),
        "body" => fs.body.clone(),
        "quote" => fs.quote.clone(),
        "code" => fs.code.clone(),
        _ => None,
    }
    .unwrap_or_default()
}

/// Write a font slot on the settings (`None` = reset to default).
fn set_slot(fs: &mut FontSettings, slot: &str, value: Option<String>) {
    match slot {
        "h1" => fs.h1 = value,
        "h2" => fs.h2 = value,
        "h3" => fs.h3 = value,
        "body" => fs.body = value,
        "quote" => fs.quote = value,
        "code" => fs.code = value,
        _ => {}
    }
}

/// Fonts from the catalog matching `category` (empty = any) and `query`
/// (empty = all), capped at 50. Reads `font_catalog()`, so calling it inside a
/// reactive `for`/`if` scrutinee makes that block refresh when the async catalog
/// arrives or the query changes.
fn filtered_fonts(category: &str, query: &str) -> Vec<fonts::FontInfo> {
    let q = query.to_lowercase();
    fonts::font_catalog()
        .get()
        .into_iter()
        .filter(|f| {
            if !category.is_empty() && f.category != category {
                return false;
            }
            q.is_empty() || f.family.to_lowercase().contains(&q)
        })
        .take(50)
        .collect()
}

/// Persist font settings: update the signal + the current book, then debounce a
/// PUT to the server. `load_book_fonts` is a documented no-op on native (glyph
/// registration is Phase 3), so the choice round-trips and saves but the desktop
/// glyphs won't change yet.
pub(super) fn save_font_settings(
    fs: FontSettings,
    font_settings: Signal<FontSettings>,
    store: AppStore,
    bid_signal: Signal<String>,
    font_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
) {
    fonts::load_book_fonts(&fs);
    font_settings.set(fs.clone());
    // Dual-write: the book document holds font settings too, and used to receive them
    // only at seed time — so typography changes silently diverged the two copies.
    crate::local_book::set_font_settings(&bid_signal.get(), &fs);
    store.current_book.update(|book| {
        if let Some(b) = book {
            b.font_settings = Some(fs.clone());
        }
    });
    if let Some(h) = font_save_timer_id.get() {
        rinch_core::clear_timeout(h);
    }
    let bid = bid_signal.get();
    // `unowned`: a debounced save must outlive the element that scheduled it. Since
    // rinch #141, a callback parked during a render is dropped if that scope is
    // disposed first — and these fire exactly when the UI is going away (the font
    // dropdown closes on select, a chapter switch tears down the editor pane). The
    // font picker lost its save this way; the rest are the same shape.
    font_save_timer_id.set(Some(rinch_core::reactive::unowned(|| rinch_core::set_timeout(500, move || {
        let req = plotweb_common::UpdateBookRequest {
            title: None,
            description: None,
            font_settings: Some(fs),
            cover_image: None,
            calendar: None,
            span_rule: None,
        };
        api::put::<_, serde_json::Value>(&format!("/api/books/{}", bid), &req, move |_result| {});
    }))));
}

/// One searchable font dropdown for a single slot — fully reactive rsx, so it
/// works on web and native alike. `open_slot` holds the slot whose dropdown is
/// currently shown; only one picker opens at a time and selecting/opening
/// another dismisses the rest (this is also our click-outside substitute — there
/// is no `onblur` to race the option click, which rinch fires on pointer-down).
#[allow(clippy::too_many_arguments)]
fn font_picker(
    __scope: &mut RenderScope,
    slot_name: &'static str,
    default_family: &'static str,
    category: &'static str,
    font_settings: Signal<FontSettings>,
    store: AppStore,
    bid_signal: Signal<String>,
    font_save_timer_id: Signal<Option<rinch_core::TimeoutHandle>>,
    open_slot: Signal<&'static str>,
) -> NodeHandle {
    let query = Signal::new(slot_family(&font_settings.get(), slot_name));
    // Keep the input in sync when `font_settings` changes externally (book load,
    // reset). `set_if_changed` no-ops on the initial run and while the user types
    // (only `query` changes then, not `font_settings`), so it never clobbers the
    // in-progress search text.
    __scope.create_effect(move || {
        query.set_if_changed(slot_family(&font_settings.get(), slot_name));
    });

    let apply = move |family: String| {
        if !family.is_empty() {
            fonts::load_single_font(&family);
        }
        let mut fs = font_settings.get();
        let value = if family.is_empty() { None } else { Some(family.clone()) };
        set_slot(&mut fs, slot_name, value);
        save_font_settings(fs, font_settings, store, bid_signal, font_save_timer_id);
        query.set(family);
        open_slot.set("");
    };

    let placeholder = format!("Search fonts... (default: {})", default_family);

    rsx! {
        div { class: "font-picker", data-font-slot: slot_name,
            input {
                r#type: "text",
                autocomplete: "off",
                placeholder: placeholder,
                value: {move || query.get()},
                oninput: move |v: String| { query.set(v); open_slot.set(slot_name); },
                onclick: move || open_slot.set(slot_name),
            }
            if open_slot.get() == slot_name {
                div { class: "font-dropdown open",
                    div {
                        class: "font-option-default",
                        data-font-value: "",
                        onclick: move || apply(String::new()),
                        {"Reset to default"}
                    }
                    for f in filtered_fonts(category, &query.get()) {
                        {font_option(__scope, f.family.clone(), f.category.clone(), apply)}
                    }
                    if fonts::font_catalog().get().is_empty() {
                        div { class: "font-loading", {"Loading fonts..."} }
                    } else if filtered_fonts(category, &query.get()).is_empty() {
                        div { class: "font-empty", {"No fonts found"} }
                    }
                }
            }
        }
    }
}

/// One font option row inside a picker dropdown. Takes owned strings so each
/// consumer (the `data-font-value` attr effect, the click handler, the label
/// text) gets its own copy without fighting rsx's move-capture.
fn font_option(
    __scope: &mut RenderScope,
    family: String,
    category: String,
    apply: impl Fn(String) + 'static,
) -> NodeHandle {
    let fam_attr = family.clone();
    let fam_click = family.clone();
    rsx! {
        div {
            class: "font-option",
            data-font-value: fam_attr,
            onclick: move || apply(fam_click.clone()),
            span { {family} }
            span { class: "font-category", {category} }
        }
    }
}

/// A reactive `<select>` for a spacing/indent setting. Native `<select>` (not the
/// `Select` component) so the e2e tests' `selectOption`/`#pw-…` locators keep
/// working; rinch delivers its `change` as an `oninput`-style `Fn(String)`.
fn typo_select(
    __scope: &mut RenderScope,
    id: &'static str,
    options: &'static [(f64, &'static str)],
    current: impl Fn() -> f64 + Copy + 'static,
    apply: impl Fn(f64) + 'static,
) -> NodeHandle {
    rsx! {
        select {
            id: id,
            class: "pw-typo-select",
            value: {move || format!("{}", current())},
            onchange: move |v: String| apply(v.parse().unwrap_or(0.0)),
            for opt in options {
                option { key: format!("{}", opt.0), value: format!("{}", opt.0), {opt.1} }
            }
        }
    }
}

/// Render the Typography pane (CSS toggle, always in DOM).
pub(in crate::pages::book) fn render(
    __scope: &mut RenderScope,
    state: BookState,
    store: AppStore,
) -> NodeHandle {
    let BookState { active_pane, font_settings, bid_signal, font_save_timer_id, open_slot, .. } = state;
    rsx! {
        div {
            class: "book-main-scroll",
            style: {move || if matches!(active_pane.get(), BookPane::Typography) { "" } else { "display:none;" }},

            div { class: "chapters-pane",
                Title { order: 3, "Typography" }
                Space { h: "md" }
                div { class: "typography-section",
                    Text { weight: "600", size: "sm", "Fonts" }
                    Space { h: "xs" }
                    div { class: "font-selector-grid", id: "font-selector-grid",
                        span { class: "font-selector-label", "Heading 1" }
                        {font_picker(__scope, "h1", "Macondo Swash Caps", "", font_settings, store, bid_signal, font_save_timer_id, open_slot)}
                        span { class: "font-selector-label", "Heading 2" }
                        {font_picker(__scope, "h2", "Macondo Swash Caps", "", font_settings, store, bid_signal, font_save_timer_id, open_slot)}
                        span { class: "font-selector-label", "Heading 3+" }
                        {font_picker(__scope, "h3", "Macondo Swash Caps", "", font_settings, store, bid_signal, font_save_timer_id, open_slot)}
                        span { class: "font-selector-label", "Body Text" }
                        {font_picker(__scope, "body", "Playwrite DE Grund", "", font_settings, store, bid_signal, font_save_timer_id, open_slot)}
                        span { class: "font-selector-label", "Blockquote" }
                        {font_picker(__scope, "quote", "inherit", "", font_settings, store, bid_signal, font_save_timer_id, open_slot)}
                        span { class: "font-selector-label", "Code" }
                        {font_picker(__scope, "code", "monospace", "monospace", font_settings, store, bid_signal, font_save_timer_id, open_slot)}
                    }
                    Space { h: "lg" }
                    Text { weight: "600", size: "sm", "Spacing" }
                    Space { h: "xs" }
                    div { class: "font-selector-grid", id: "spacing-selector-grid",
                        span { class: "font-selector-label", "Paragraph" }
                        {typo_select(__scope, "pw-paragraph-spacing", SPACING_OPTIONS,
                            move || font_settings.get().paragraph_spacing.unwrap_or(8.0),
                            move |val: f64| {
                                let mut fs = font_settings.get();
                                fs.paragraph_spacing = if (val - 8.0).abs() < 0.01 { None } else { Some(val) };
                                save_font_settings(fs, font_settings, store, bid_signal, font_save_timer_id);
                            })}
                        span { class: "font-selector-label", "Body Indent" }
                        {typo_select(__scope, "pw-paragraph-indent", INDENT_OPTIONS,
                            move || font_settings.get().paragraph_indent.unwrap_or(0.0),
                            move |val: f64| {
                                let mut fs = font_settings.get();
                                fs.paragraph_indent = if val.abs() < 0.01 { None } else { Some(val) };
                                save_font_settings(fs, font_settings, store, bid_signal, font_save_timer_id);
                            })}
                        span { class: "font-selector-label", "Heading Indent" }
                        {typo_select(__scope, "pw-heading-indent", INDENT_OPTIONS,
                            move || font_settings.get().heading_indent.unwrap_or(0.0),
                            move |val: f64| {
                                let mut fs = font_settings.get();
                                fs.heading_indent = if val.abs() < 0.01 { None } else { Some(val) };
                                save_font_settings(fs, font_settings, store, bid_signal, font_save_timer_id);
                            })}
                    }
                    Space { h: "lg" }
                    Text { weight: "600", size: "sm", "Writing" }
                    Space { h: "xs" }
                    // Per *device*, not per account — a manuscript proofread on a
                    // laptop and drafted on a phone wants different answers. See
                    // `crate::spell::settings`.
                    div { class: "typo-switch-row", id: "pw-spellcheck",
                        Switch {
                            label: "Spellcheck",
                            description: "Underline misspelled words while you write. Remembered on this device.",
                            checked_fn: move || store.spellcheck_enabled.get(),
                            onchange: move || {
                                let next = !store.spellcheck_enabled.get();
                                store.spellcheck_enabled.set(next);
                                crate::spell::settings::persist(next);
                            },
                        }
                    }
                    Space { h: "lg" }
                    Text { size: "sm", color: "dimmed", "Preview:" }
                    div { class: "font-preview-box", id: "font-preview",
                        h3 {
                            style: {move || {
                                let fs = font_settings.get();
                                format!("font-family: '{}', cursive; text-indent: {}px",
                                    fs.h1.as_deref().unwrap_or("Macondo Swash Caps"),
                                    fs.heading_indent.unwrap_or(0.0))
                            }},
                            {"Chapter Title"}
                        }
                        h4 {
                            style: {move || {
                                let fs = font_settings.get();
                                format!("font-family: '{}', cursive; text-indent: {}px",
                                    fs.h2.as_deref().unwrap_or("Macondo Swash Caps"),
                                    fs.heading_indent.unwrap_or(0.0))
                            }},
                            {"Scene Heading"}
                        }
                        p {
                            class: "preview-body",
                            style: {move || {
                                let fs = font_settings.get();
                                format!("font-family: '{}', serif; margin-bottom: {}px; text-indent: {}px",
                                    fs.body.as_deref().unwrap_or("Playwrite DE Grund"),
                                    fs.paragraph_spacing.unwrap_or(8.0),
                                    fs.paragraph_indent.unwrap_or(0.0))
                            }},
                            {"The quick brown fox jumps over the lazy dog. She stared out the window, watching the rain trace paths down the glass."}
                        }
                        p {
                            class: "preview-body",
                            style: {move || {
                                let fs = font_settings.get();
                                format!("font-family: '{}', serif; margin-bottom: {}px; text-indent: {}px",
                                    fs.body.as_deref().unwrap_or("Playwrite DE Grund"),
                                    fs.paragraph_spacing.unwrap_or(8.0),
                                    fs.paragraph_indent.unwrap_or(0.0))
                            }},
                            {"The next morning, she found the letter on the doorstep, its edges curled and damp from the night air."}
                        }
                        blockquote {
                            style: {move || {
                                let fs = font_settings.get();
                                format!("font-family: '{}', serif", fs.quote.as_deref().unwrap_or("inherit"))
                            }},
                            {"\u{201c}All that glitters is not gold.\u{201d}"}
                        }
                        p {
                            code {
                                style: {move || {
                                    let fs = font_settings.get();
                                    format!("font-family: '{}', monospace", fs.code.as_deref().unwrap_or("monospace"))
                                }},
                                {"const story = new Adventure();"}
                            }
                        }
                    }
                }
            }
        }
    }
}
