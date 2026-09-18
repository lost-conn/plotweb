use rinch::prelude::*;

/// Tier 3 of the book page's overlay tiers (see `design/01-language.html#overlays`):
/// a right-hand slide-over for long or multi-step work — Import, Export, Book
/// settings, Create/Edit beta link — that keeps the manuscript visible behind
/// it. Wraps rinch's `Drawer` rather than hand-rolling a panel: `Drawer` already
/// stays mounted while closed so its slide-in transitions on both backends
/// (rinch #761/60d28b9), and its dismiss stack gives Escape and outside-click
/// dismiss for free (rinch #672/b1b47ca) — this component does not reimplement
/// either.
///
/// Below 768px this becomes a bottom sheet (`SHEET_CSS`), matching the
/// existing precedent in the editor's feedback rail
/// (`.editor-feedback-sidebar`: `border-radius: 12px 12px 0 0`, `60vh`) rather
/// than inventing a second mobile treatment.
///
/// See [`crate::components::dialog::Dialog`] for the same `opened_fn`
/// forwarding rationale — a `_fn`-suffixed prop re-closures rather than
/// passing through directly, so toggling this sheet doesn't tear down and
/// rebuild the `Drawer` subtree.
#[component]
pub fn Sheet(
    opened_fn: Option<std::rc::Rc<dyn Fn() -> bool>>,
    onclose: Callback,
    title: String,
    children: &[NodeHandle],
) -> NodeHandle {
    let body = rsx! { div { class: "pw-sheet-body" } };
    for child in children {
        body.append_child(child);
    }

    rsx! {
        Drawer {
            opened_fn: {
                let opened_fn = opened_fn.clone();
                move || opened_fn.as_ref().is_some_and(|f| f())
            },
            onclose: move || onclose.invoke(),
            title: title.clone(),
            position: "right",
            size: "md",
            class: "pw-sheet",

            {body}
        }
    }
}

pub const SHEET_CSS: &str = r#"
/* `class: "pw-sheet"` on `Sheet`'s inner `Drawer` component lands on
   `Drawer::render()`'s returned node — `.rinch-drawer__root` (the fixed,
   full-viewport positioning context) — not on the `.rinch-drawer` panel
   inside it (component `class:`/`style:` apply to the node the component
   itself returns, not to whichever descendant a reader might expect).
   Every rule below is scoped as a `.pw-sheet` descendant selector for that
   reason, including the mobile override — a same-element `.pw-sheet.rinch-
   drawer` selector silently never matches anything. */
.pw-sheet .rinch-drawer {
    box-shadow: var(--pw-shadow-3);
}

.pw-sheet .rinch-drawer__header {
    font-family: var(--pw-font-display);
}

.pw-sheet .rinch-drawer__title {
    font-family: var(--pw-font-display);
    font-weight: 400;
    font-size: var(--pw-text-lg);
}

.pw-sheet-body {
    display: flex;
    flex-direction: column;
    gap: var(--pw-space-sm);
}

@media (max-width: 768px) {
    /* Bottom sheet on mobile — same treatment as the editor's feedback rail
       (`.editor-feedback-sidebar`): full-width, docked to the bottom, top
       corners only. */
    .pw-sheet .rinch-drawer {
        top: auto !important;
        bottom: 0 !important;
        left: 0 !important;
        right: 0 !important;
        width: 100% !important;
        min-width: 100% !important;
        max-width: 100% !important;
        height: 85vh !important;
        max-height: 85vh !important;
        border-radius: var(--pw-radius-lg) var(--pw-radius-lg) 0 0;
        border-left: none;
        border-top: 1px solid var(--rinch-color-border);
        transform: translateY(100%);
    }

    .pw-sheet .rinch-drawer.rinch-drawer--opened {
        transform: translateY(0);
    }
}
"#;
