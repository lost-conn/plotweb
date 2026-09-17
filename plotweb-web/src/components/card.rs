use rinch::prelude::*;

/// The general-purpose surface primitive: a rounded, bordered panel on
/// `var(--rinch-color-surface)`. Not the dashboard's book jacket (see
/// [`super::book_jacket::BookJacket`]) — this is the plain container other
/// pages reach for (currently just the "new book" tile), extracted here so the
/// next primitive stage (Stage 4's `Sheet`/`Dialog`) has a shared base rather
/// than each pane hand-rolling its own `border-radius`/`box-shadow` pair.
///
/// `interactive` adds the hover lift + shadow-3 used by clickable cards (the
/// jacket borrows the same transition, tuned per the `#dash` mockup).
/// `dashed` swaps the border style for the "add new" affordance.
#[component]
pub fn Card(
    interactive: bool,
    dashed: bool,
    class: String,
    style: String,
    onclick: Callback,
    children: &[NodeHandle],
) -> NodeHandle {
    let mut classes = vec!["pw-card"];
    if interactive {
        classes.push("pw-card--interactive");
    }
    if dashed {
        classes.push("pw-card--dashed");
    }
    if !class.is_empty() {
        classes.push(&class);
    }
    let class_str = classes.join(" ");

    let card = rsx! {
        div {
            class: class_str,
            style: style,
            onclick: move || onclick.invoke(),
        }
    };
    for child in children {
        card.append_child(child);
    }
    card
}

const CARD_CSS: &str = r#"
.pw-card {
    background: var(--rinch-color-surface);
    border-radius: var(--pw-radius-md);
}

.pw-card--interactive {
    cursor: pointer;
    transition: border-color var(--pw-dur-fast) var(--pw-ease),
        color var(--pw-dur-fast) var(--pw-ease);
}

.pw-card--dashed {
    background: transparent;
    border: 1px dashed var(--rinch-color-border);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--pw-space-xs);
    color: var(--rinch-color-dimmed);
}

.pw-card--dashed.pw-card--interactive:hover {
    border-color: var(--rinch-color-teal-7);
    color: var(--rinch-color-teal-4);
}
"#;

/// Injects [`CARD_CSS`] once. Call from any page that mounts a `Card`.
pub fn card_styles() -> &'static str {
    CARD_CSS
}
