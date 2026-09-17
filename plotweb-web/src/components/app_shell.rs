use rinch::prelude::*;
use rinch_core::use_store;

use crate::store::{AppStore, Route};

/// The design scales that do not change with the theme.
///
/// Split from [`warm_overrides`] because only colour and elevation depend on light/dark;
/// these are constant, so they are emitted once rather than rebuilt on every toggle.
///
/// Each scale replaces a pile of literals that had grown up page by page — at the time
/// of writing, 10 distinct font sizes, 13 padding values, 8 border-radius treatments and
/// 5 unscaled z-indexes, with tokens and literals mixed inside single rules. Nothing
/// consumes these yet; adopting them is the work of the page passes that follow, so this
/// block is deliberately inert and must not change a single pixel on its own.
const DESIGN_TOKENS: &str = r#"
:root {
  /* ── Type ──
     Six sizes, each with a job. Previously 11/12/13px all did "small secondary
     text" duty interchangeably, which is why the same kind of label rendered at
     three sizes depending on which page you were on. */
  --pw-text-2xs: 11px;   /* counts, uppercase micro labels */
  --pw-text-xs:  12px;   /* metadata, captions, timestamps */
  --pw-text-sm:  13px;   /* dense UI: sidebar, buttons, inputs */
  --pw-text-md:  15px;   /* default body UI */
  --pw-text-lg:  19px;   /* pane heading */
  --pw-text-xl:  26px;   /* page title */

  --pw-lh-tight: 1.25;
  --pw-lh-ui:    1.45;
  --pw-lh-prose: 1.7;

  /* ── Space — 4px base. Retires 6, 10, 14, 20 and 40px. ── */
  --pw-space-3xs: 2px;
  --pw-space-2xs: 4px;
  --pw-space-xs:  8px;
  --pw-space-sm:  12px;
  --pw-space-md:  16px;
  --pw-space-lg:  24px;
  --pw-space-xl:  32px;
  --pw-space-2xl: 48px;
  --pw-space-3xl: 64px;

  /* ── Radius ──
     Three plus a pill, down from eight treatments. Four different radii (3, 4, 6
     and 8px) were describing the same "small box" role. */
  --pw-radius-sm:   4px;    /* inputs, chips, toolbar buttons */
  --pw-radius-md:   8px;    /* cards, panels, dialogs */
  --pw-radius-lg:   14px;   /* covers, mobile sheets */
  --pw-radius-full: 999px;

  /* ── Z-index ──
     Named steps, so the ordering is stated rather than rediscovered. This also
     fixes a live bug: the font dropdown sat at 100 and the mobile sidebar at 200,
     so opening the sidebar over an open font picker hid the picker behind it. */
  --pw-z-sticky:  10;    /* pane headers */
  --pw-z-popover: 100;   /* dropdowns, selection toolbars */
  --pw-z-overlay: 200;   /* backdrops */
  --pw-z-sheet:   210;   /* drawers, side sheets */
  --pw-z-dialog:  220;   /* centred dialogs */
  --pw-z-drag:    300;   /* drag ghost */
  --pw-z-toast:   400;   /* transient messages */

  /* ── Motion ── */
  --pw-ease:     cubic-bezier(.2, .8, .2, 1);
  --pw-dur-fast: 120ms;
  --pw-dur:      200ms;
  --pw-dur-slow: 320ms;

  /* ── Measure ──
     Prose sits at roughly 68 characters wherever it appears. The reader already
     does this at 760px; the editor runs the full width of its pane, which is the
     one number most worth fixing in the passes that follow. */
  --pw-measure:  34em;
  --pw-pane-max: 720px;

  /* ── Fonts ──
     Macondo and Playwrite keep the book: titles, chapter headings, prose. The UI
     face is separate because the Typography panel only restyles *book content* —
     application chrome is not user-configurable, so it has to hold up at 11px. */
  --pw-font-display: 'Macondo Swash Caps', cursive;
  --pw-font-prose:   'Playwrite DE Grund', Georgia, 'Times New Roman', serif;
  --pw-font-ui:      'Source Sans 3', system-ui, -apple-system, sans-serif;

  /* Focus is not elevation. It reads `--rinch-color-body` at use time, so one
     definition serves both themes. */
  --pw-focus-ring: 0 0 0 2px var(--rinch-color-body), 0 0 0 4px var(--rinch-color-teal-6);
}
"#;

/// Returns a `:root { ... }` CSS block with warm color overrides for the given mode.
///
/// Also carries the elevation scale, which cannot live in [`DESIGN_TOKENS`]: a shadow
/// legible on `#FAF8F5` disappears entirely on `#1C1917`, so dark mode needs several
/// times the opacity to read at all.
fn warm_overrides(dark: bool) -> String {
    let font = "--rinch-font-family: 'Playwrite DE Grund', Georgia, 'Times New Roman', serif;";
    if dark {
        format!(":root {{
  {font}
  --rinch-color-body: #1C1917;
  --rinch-color-surface: #262220;
  --rinch-color-text: #E7E0D8;
  --rinch-color-dimmed: #9C9489;
  --rinch-color-border: #3D3733;
  --rinch-color-placeholder: #6B6359;
  --pw-color-deep: #1A1714;
  --pw-color-deepest: #14120F;
  --pw-hairline: rgba(231, 224, 216, 0.09);
  --pw-shadow-1: 0 1px 2px rgba(0, 0, 0, 0.4);
  --pw-shadow-2: 0 4px 16px -2px rgba(0, 0, 0, 0.55);
  --pw-shadow-3: 0 18px 50px -10px rgba(0, 0, 0, 0.7);
}}")
    } else {
        format!(":root {{
  {font}
  --rinch-color-body: #FAF8F5;
  --rinch-color-surface: #FFFFFF;
  --rinch-color-text: #2C2520;
  --rinch-color-dimmed: #8C8278;
  --rinch-color-border: #E0D8CF;
  --rinch-color-placeholder: #A89E94;
  --pw-color-deep: #F3F0EC;
  --pw-color-deepest: #EDE9E3;
  --pw-hairline: rgba(44, 37, 32, 0.09);
  --pw-shadow-1: 0 1px 2px rgba(60, 45, 30, 0.07);
  --pw-shadow-2: 0 4px 16px -2px rgba(60, 45, 30, 0.13);
  --pw-shadow-3: 0 18px 50px -10px rgba(60, 45, 30, 0.22);
}}")
    }
}

/// CSS for the app shell layout.
const APP_SHELL_CSS: &str = r#"
* {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}

html, body {
    height: 100dvh;
    font-family: var(--rinch-font-family);
    background: var(--rinch-color-body);
    color: var(--rinch-color-text);
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
}

h1, h2, h3, h4, h5, h6, .rinch-title {
    font-family: 'Macondo Swash Caps', cursive;
}

/* ── Auth Pages ────────────────────────────────────────── */

.auth-page {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100dvh;
    background: radial-gradient(90% 70% at 50% 0%, var(--pw-color-deep), var(--rinch-color-body));
    font-family: var(--pw-font-ui);
}

.auth-card {
    width: 360px;
    padding: 34px 32px;
}

.auth-mark {
    display: flex;
    flex-direction: column;
    align-items: center;
    margin-bottom: var(--pw-space-lg);
}

.auth-mark h3.rinch-title {
    font-family: var(--pw-font-display);
    font-weight: 400;
    font-size: 22px;
    color: var(--rinch-color-teal-4);
}

.auth-mark .rinch-text {
    margin-top: 3px;
}

/* PasswordInput is a separate component with its own block, not a variant of
   TextInput — so every one of these selectors has to name both. Styling only the
   text-input half left the password fields on login, register and reset-password
   rendering in the book's handwriting face at 14px, beside a 13px Source Sans
   username box. */
.auth-card .rinch-text-input__label,
.auth-card .rinch-password-input__label,
.auth-card .rinch-checkbox__label {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
}

/* TextInput draws its own border on the input; PasswordInput draws it on a
   wrapper that also holds the reveal toggle. Giving the password *input* a
   border too nests a box inside a box and visually detaches the eye icon, so
   the two take the chrome at different levels: the field itself for TextInput,
   the wrapper for PasswordInput. */
.auth-card .rinch-text-input__input,
.auth-card .rinch-password-input__wrapper {
    border-radius: var(--pw-radius-sm);
    background: var(--rinch-color-surface);
    border: 1px solid var(--rinch-color-border);
}

.auth-card .rinch-text-input__input,
.auth-card .rinch-password-input__input {
    font-family: var(--pw-font-ui);
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
}

.auth-card .rinch-password-input__input {
    background: transparent;
    border: none;
}

.auth-card .rinch-text-input__input:focus,
.auth-card .rinch-password-input__wrapper:focus-within {
    outline: none;
    border-color: var(--rinch-color-teal-7);
    box-shadow: var(--pw-focus-ring);
}

.auth-card .rinch-password-input__input:focus {
    outline: none;
}

.auth-page .rinch-alert,
.auth-card .rinch-btn,
.auth-card button {
    font-family: var(--pw-font-ui);
}

/* Alert ships a light-mode palette with a hardcoded pale background, so on a dark
   page it lands as a bright white-blue slab. It is also always blue here: the call
   sites ask for `color: "teal"`, which rinch's AlertColor does not parse, so it
   falls back. Rather than chase a colour name that happens to parse, the auth
   alerts are restated in tokens — quiet block, accent on the left edge, which is
   the same treatment feedback quotes get elsewhere in the design. */
.auth-card .rinch-alert {
    background: var(--pw-color-deep);
    border: 1px solid var(--rinch-color-border);
    border-left: 2px solid var(--rinch-color-teal-7);
    border-radius: var(--pw-radius-sm);
    color: var(--rinch-color-text);
}

.auth-card .rinch-alert--red {
    border-left-color: var(--rinch-color-red-6);
}

/* Rinch colours these from `.rinch-alert--blue .rinch-alert__title`, which ties
   this rule on specificity and wins on document order. Matching the alert class
   as well outweighs it without reaching for !important. */
.auth-page .auth-card .rinch-alert .rinch-alert__wrapper,
.auth-page .auth-card .rinch-alert .rinch-alert__message {
    color: var(--rinch-color-dimmed);
    font-size: var(--pw-text-xs);
}

.auth-page .auth-card .rinch-alert .rinch-alert__title {
    color: var(--rinch-color-text);
    font-size: var(--pw-text-sm);
}

.auth-row {
    display: flex;
    align-items: center;
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
}

.auth-row .sp {
    flex: 1;
}

.auth-foot {
    text-align: center;
    margin-top: var(--pw-space-md);
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
}

/* ── Scrollbar ─────────────────────────────────────────── */

::-webkit-scrollbar {
    width: 8px;
}

::-webkit-scrollbar-track {
    background: transparent;
}

::-webkit-scrollbar-thumb {
    background: var(--rinch-color-border);
    border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
    background: var(--rinch-color-dimmed);
}

/* ── Selection ─────────────────────────────────────────── */

::selection {
    background: var(--rinch-color-teal-8);
    color: var(--rinch-color-teal-1);
}

/* ── Auth responsive ───────────────────────────────────── */

@media (max-width: 480px) {
    .auth-card {
        width: 100% !important;
        margin: 0 16px;
    }
}

/* ── Reduced motion ────────────────────────────────────── */

/* The motion tokens exist so that movement is deliberate; this makes it optional.
   Affects only users who have asked the OS for less animation. */
@media (prefers-reduced-motion: reduce) {
    *, *::before, *::after {
        animation-duration: 0.01ms !important;
        animation-iteration-count: 1 !important;
        transition-duration: 0.01ms !important;
        scroll-behavior: auto !important;
    }
}
"#;

/// A label an author can recognise: the chapter's title if we know it, else the id.
fn rescue_label(store: &AppStore, doc_id: &str) -> String {
    if let Some(id) = doc_id.strip_prefix("chapter:") {
        if let Some(ch) = store.chapters.get().iter().find(|c| c.id == id) {
            return format!("chapter \"{}\"", ch.title);
        }
        return format!("a chapter ({})", &id[..id.len().min(8)]);
    }
    if let Some(id) = doc_id.strip_prefix("note:") {
        return format!("a note ({})", &id[..id.len().min(8)]);
    }
    doc_id.to_string()
}

/// Paragraphs of the live chapter/note this rescue belongs to, if it is open.
///
/// The comparison base. Deliberately *only* what is already loaded: a rescue viewer
/// that fetches is a viewer that can fail, and this has to work on a device that cannot
/// reach the server at all — which is the device most likely to be holding one.
fn live_paragraphs(store: &AppStore, doc_id: &str) -> Vec<String> {
    match store.open_body.get() {
        Some((open_id, text)) if open_id == doc_id => paragraphs_of(&text),
        // A different chapter is open, or none. Comparing against the wrong document
        // would mark every paragraph as missing, which is worse than not comparing.
        _ => Vec::new(),
    }
}

/// Split a stored `DocNode` JSON (or legacy HTML) into comparable paragraphs.
fn paragraphs_of(content: &str) -> Vec<String> {
    rescue_preview(content)
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect()
}

/// The rescued copy as rows: each paragraph, and whether it is missing from the live
/// document. `None` for the flag when there is nothing to compare against.
fn rescue_rows(store: &AppStore) -> Vec<(String, bool)> {
    let Some(json) = store.rescue_text.get() else {
        return Vec::new();
    };
    let live = store
        .rescue_open
        .get()
        .map(|(doc_id, _)| live_paragraphs(store, &doc_id))
        .unwrap_or_default();
    paragraphs_of(&json)
        .into_iter()
        .map(|para| {
            let only_here = !live.is_empty() && !live.iter().any(|l| l.contains(para.as_str()));
            (para, only_here)
        })
        .collect()
}

/// Plain text out of stored content, for a read-only preview.
///
/// Handles both shapes on purpose: the editor's `DocNode` JSON, and the legacy
/// HTML/Markdown that predates it and is still in older chapters. A comparison base
/// that silently returned nothing for legacy content would report "nothing to compare
/// against" for exactly the oldest, most valuable chapters.
fn rescue_preview(content: &str) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(map) => {
                if map.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = map.get("text").and_then(|t| t.as_str()) {
                        out.push_str(t);
                    }
                }
                if let Some(kids) = map.get("content") {
                    walk(kids, out);
                    // Paragraph boundaries the author will expect to see.
                    if map.get("type").and_then(|t| t.as_str()) == Some("paragraph") {
                        out.push_str("\n\n");
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        let mut out = String::new();
        walk(&v, &mut out);
        return out.trim().to_string();
    }
    // Legacy: block tags become paragraph breaks, everything else is stripped.
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for c in content.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' => {
                in_tag = false;
                let name = tag.trim_start_matches('/').trim();
                if name.starts_with('p')
                    || name.starts_with("br")
                    || name.starts_with('h')
                    || name.starts_with("div")
                    || name.starts_with("li")
                {
                    out.push_str("\n\n");
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// Tells the author this device kept work the server replaced, and lets them read it.
///
/// The §D8 resolution used to discard that work silently; it is preserved now
/// (`local_store::preserve_local_copy`), but a rescue nothing surfaces is only
/// marginally better than no rescue at all — the 2026-08-29 loss was invisible until
/// the word count dropped the next morning.
#[component]
fn rescue_banner(store: AppStore) -> NodeHandle {
    let open_rescue = move |doc_id: String, slot: String| {
        move || {
            let (d, s) = (doc_id.clone(), slot.clone());
            store.rescue_open.set(Some((d.clone(), s.clone())));
            store.rescue_text.set(None);
            crate::local_store::spawn(async move {
                match crate::local_store::materialize_rescued_copy(&d, &s).await {
                    Ok(Some(json)) => store.rescue_text.set(Some(json)),
                    Ok(None) => store.rescue_text.set(Some(String::new())),
                    Err(e) => {
                        log::warn!("local-first: {d}: {e}");
                        store.rescue_text.set(Some(String::new()));
                    }
                }
            });
        }
    };

    rsx! {
        div {
            style: "background: #7C4A2D; color: #FFF6EC; padding: 10px 16px; display: flex; align-items: center; gap: 12px; font-size: 14px; flex-wrap: wrap;",

            span {
                {move || {
                    let n = store.rescued.get().len();
                    let what = if n == 1 { "a copy" } else { "copies" };
                    format!("This device kept {what} of work that never reached the server ({n}).")
                }}
            }

            for (doc_id, slot) in store.rescued.get() {
                button {
                    key: format!("{doc_id}/{slot}"),
                    style: "background: transparent; color: #FFF6EC; border: 1px solid #FFF6EC; border-radius: 4px; padding: 3px 10px; cursor: pointer; font-size: 13px;",
                    onclick: open_rescue(doc_id.clone(), slot.clone()),
                    {format!("View {}", rescue_label(&store, &doc_id))}
                }
            }
        }
    }
}

/// The rescued text, read-only, with the two things an author needs: to read it, and
/// to say they are done with it.
#[component]
fn rescue_viewer(store: AppStore) -> NodeHandle {
    let close_viewer = move || {
        store.rescue_open.set(None);
    };
    let discard = move || {
        let Some((doc_id, slot)) = store.rescue_open.get() else {
            return;
        };
        store.rescue_open.set(None);
        crate::local_store::spawn(async move {
            if let Err(e) = crate::local_store::discard_rescued_copy(&doc_id, &slot).await {
                log::warn!("local-first: {doc_id}: discard failed: {e}");
            }
            match crate::local_store::rescued_copies().await {
                Ok(found) => store.rescued.set(found),
                Err(e) => log::warn!("local-first: could not re-list rescued copies: {e}"),
            }
        });
    };

    rsx! {
        div {
            style: "position: fixed; inset: 0; background: rgba(0,0,0,0.55); z-index: 1000; display: flex; align-items: center; justify-content: center; padding: 24px;",

            div {
                style: "background: var(--rinch-color-surface); color: var(--rinch-color-text); border-radius: 8px; max-width: 760px; width: 100%; max-height: 80dvh; display: flex; flex-direction: column; overflow: hidden;",

                div {
                    style: "padding: 14px 18px; border-bottom: 1px solid var(--rinch-color-border);",
                    div { style: "font-weight: 600;", "Kept on this device" }
                    div {
                        style: "font-size: 13px; color: var(--rinch-color-dimmed); margin-top: 2px;",
                        "This text was in this browser when the server replaced the document. Copy anything you still want, then discard it."
                    }
                }

                div {
                    style: "padding: 6px 18px 0; font-size: 13px; color: var(--rinch-color-dimmed);",
                    {move || {
                        let Some(json) = store.rescue_text.get() else {
                            return String::new();
                        };
                        let Some((doc_id, _)) = store.rescue_open.get() else {
                            return String::new();
                        };
                        let live = live_paragraphs(&store, &doc_id);
                        if live.is_empty() {
                            return "The chapter this came from is not open, so there is \
                                    nothing to compare it against — every paragraph is shown."
                                .to_string();
                        }
                        let only_here = paragraphs_of(&json)
                            .into_iter()
                            .filter(|p| !live.iter().any(|l| l.contains(p.as_str())))
                            .count();
                        match only_here {
                            0 => "Everything in this copy is already in the chapter — \
                                  nothing here is missing from it."
                                .to_string(),
                            1 => "1 paragraph here is not in the chapter (highlighted).".to_string(),
                            n => format!("{n} paragraphs here are not in the chapter (highlighted)."),
                        }
                    }}
                }

                div {
                    style: "padding: 12px 18px 16px; overflow-y: auto; line-height: 1.55; flex: 1;",
                    // A reactive node swap has to be a match in the macro — a closure
                    // returning a node renders as its Debug text here.
                    match store.rescue_text.get() {
                        None => div { "Reading…" },
                        Some(json) if json.is_empty() => div {
                            "This copy could not be projected back to text. The bytes are still stored on this device."
                        },
                        Some(_) => div {
                            for (para, only_here) in rescue_rows(&store) {
                                div {
                                    key: para.clone(),
                                    // The only-here paragraphs are the reason this
                                    // viewer exists; the rest is context for them.
                                    style: {
                                        if only_here {
                                            "margin-bottom: 12px; padding: 6px 10px; border-left: 3px solid #C97B4A; background: rgba(201,123,74,0.12);"
                                        } else {
                                            "margin-bottom: 12px; padding: 6px 10px; opacity: 0.55;"
                                        }
                                    },
                                    {para}
                                }
                            }
                        },
                    }
                }

                div {
                    style: "padding: 12px 18px; border-top: 1px solid var(--rinch-color-border); display: flex; gap: 10px; justify-content: flex-end;",
                    button {
                        style: "padding: 6px 14px; cursor: pointer;",
                        onclick: close_viewer,
                        "Close"
                    }
                    button {
                        style: "padding: 6px 14px; cursor: pointer; color: #B3452B;",
                        onclick: discard,
                        "Discard this copy"
                    }
                }
            }
        }
    }
}

/// Says plainly that this device is writing only to itself.
///
/// A cut-over book takes edits through sync; with sync off, a save reaches this
/// device's storage and stops there. That was true before this banner existed and
/// nothing said it, which is the shape of every loss in this arc: the app reporting
/// success for something that only half happened.
#[cfg(test)]
mod rescue_comparison {
    use super::*;

    /// The viewer's job is to show what the chapter is missing, not to dump text. A
    /// rescue whose every paragraph is already in the chapter has nothing to act on;
    /// one that carries a paragraph the chapter lacks is the whole reason it was kept.
    #[test]
    fn a_paragraph_the_chapter_lacks_is_the_one_worth_showing() {
        let doc = r#"{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"The bus section is dug in."}]},
            {"type":"paragraph","content":[{"type":"text","text":"He looks up at the night sky."}]}
        ]}"#;
        let paras = paragraphs_of(doc);
        assert_eq!(paras.len(), 2);

        // The chapter as the server has it — it stops before the second paragraph.
        let live = paragraphs_of(
            r#"{"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"The bus section is dug in."}]}
            ]}"#,
        );

        let missing: Vec<&String> = paras
            .iter()
            .filter(|p| !live.iter().any(|l| l.contains(p.as_str())))
            .collect();
        assert_eq!(missing.len(), 1);
        assert!(missing[0].contains("night sky"));
    }

    /// Legacy chapters store HTML, not `DocNode` JSON. A comparison base that returned
    /// nothing for those would report "nothing to compare against" for exactly the
    /// oldest chapters — which is where a rescue matters most.
    #[test]
    fn legacy_html_content_still_yields_paragraphs() {
        let paras = paragraphs_of("<p>First paragraph.</p><p>Second paragraph.</p>");
        assert_eq!(paras.len(), 2, "got {paras:?}");
        assert!(paras[0].contains("First paragraph."));
        assert!(paras[1].contains("Second paragraph."));
    }

    /// Whitespace differences are not content differences — the projection and the
    /// stored copy will not agree on them, and a viewer that highlighted every
    /// paragraph would be no more useful than the raw dump it replaced.
    #[test]
    fn spacing_alone_is_not_a_difference() {
        let a = paragraphs_of(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"One   two\n three"}]}]}"#,
        );
        let b = paragraphs_of(
            r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"One two three"}]}]}"#,
        );
        assert_eq!(a, b, "paragraphs compare on words, not on spacing");
    }
}

#[component]
/// Shown when a cut-over book is open on a device where sync cannot carry it.
///
/// Cutover now switches sync on by itself, so this is reachable only where the flag
/// says `plotweb_sync=0` — the kill switch. It stays because that state still has to
/// be visible: a save that reaches nothing must not look like any other save.
fn sync_off_banner() -> NodeHandle {
    rsx! {
        div {
            style: "background: #4A3B22; color: #FFF4DF; padding: 8px 16px; font-size: 13px; display: flex; align-items: center; gap: 10px;",
            span { "This book syncs, and sync is switched off on this device — anything you write here stays on this device until it is turned back on." }
        }
    }
}

#[component]
pub fn app_shell() -> NodeHandle {
    let store = use_store::<AppStore>();

    rsx! {
        Fragment {
            style { {DESIGN_TOKENS} }
            style { {|| warm_overrides(store.dark_mode.get())} }
            style { {APP_SHELL_CSS} }

            if !store.rescued.get().is_empty() {
                {rescue_banner(__scope, store)}
            }
            if store.rescue_open.get().is_some() {
                {rescue_viewer(__scope, store)}
            }
            if store
                .current_book
                .get()
                .is_some_and(|b| b.cutover && !crate::sync::enabled_for_book(&b.id))
            {
                {sync_off_banner(__scope)}
            }

            match store.current_route.get() {
                Route::Login => div {
                    {crate::pages::login::login_page(__scope)}
                },
                Route::Register => div {
                    {crate::pages::register::register_page(__scope)}
                },
                Route::ForgotPassword => div {
                    {crate::pages::forgot_password::forgot_password_page(__scope)}
                },
                Route::ResetPassword(_token) => div {
                    {crate::pages::reset_password::reset_password_page(__scope, _token)}
                },
                Route::ThemePreview => div {
                    style: "height: 100dvh; overflow-y: auto; padding: 40px 48px; background: var(--rinch-color-body);",
                    {crate::pages::theme_preview::theme_preview_page(__scope)}
                },
                Route::EditorSpike => div {
                    style: "height: 100dvh; overflow-y: auto; background: var(--rinch-color-body);",
                    {crate::pages::editor_spike::editor_spike_page(__scope)}
                },
                Route::OpfsSpike => div {
                    style: "height: 100dvh; overflow-y: auto; background: var(--rinch-color-body);",
                    {crate::pages::opfs_spike::opfs_spike_page(__scope)}
                },
                Route::Dashboard => div {
                    style: "height: 100dvh; display: flex; flex-direction: column; overflow: hidden;",
                    {crate::pages::dashboard::dashboard_page(__scope)}
                },
                Route::Book(_id) => div {
                    style: "height: 100dvh; overflow: hidden;",
                    {crate::pages::book::book_page(__scope, _id)}
                },
                Route::Reader(_token) => div {
                    style: "height: 100dvh; overflow: hidden;",
                    {crate::pages::reader::reader_page(__scope, _token)}
                },
                Route::ReaderPreview(_book_id) => div {
                    style: "height: 100dvh; overflow: hidden;",
                    {crate::pages::reader::reader_preview_page(__scope, _book_id)}
                },
            }
        }
    }
}
