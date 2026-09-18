use rinch::prelude::*;

/// The sidebar's organising device (`.pw-section-header` in the mockup):
/// uppercase, letterspaced, dimmed label with an optional count pushed to
/// the right by `margin-left: auto`. Used above the chapter list and above
/// the notes tree so both read as sections of one manuscript nav rather
/// than as flat buttons.
///
/// `active` mirrors the existing `.sidebar-section-header.active` treatment
/// (teal text) for whichever section currently owns the main pane.
/// `onclick` is optional — the notes/typography/beta/history headers all
/// navigate on click, but a bare "Manuscript" label over the chapter list
/// does not need to be clickable itself (the collapse caret already is).
#[component]
pub fn SectionHeader(
    label: String,
    count: Option<i64>,
    active: bool,
    style: String,
    onclick: Callback,
    children: &[NodeHandle],
) -> NodeHandle {
    let mut classes = vec!["pw-section-header"];
    if active {
        classes.push("pw-section-header--active");
    }
    let class_str = classes.join(" ");

    let header = rsx! {
        div {
            class: class_str,
            style: style,
            onclick: move || onclick.invoke(),
            span { class: "pw-section-header-label", {label.clone()} }
        }
    };
    for child in children {
        header.append_child(child);
    }
    if let Some(n) = count {
        header.append_child(&rsx! {
            span { class: "count", {format!("{n}")} }
        });
    }
    header
}

pub const SECTION_HEADER_CSS: &str = r#"
.pw-section-header {
    display: flex;
    align-items: center;
    gap: var(--pw-space-xs);
    font-size: var(--pw-text-2xs);
    font-weight: 600;
    letter-spacing: .08em;
    text-transform: uppercase;
    color: var(--rinch-color-dimmed);
    padding: var(--pw-space-xs) var(--pw-space-xs) var(--pw-space-2xs);
    cursor: pointer;
    transition: color var(--pw-dur-fast) var(--pw-ease);
}

.pw-section-header:hover {
    color: var(--rinch-color-text);
}

.pw-section-header--active {
    color: var(--rinch-color-teal-4);
}

.pw-section-header-label {
    flex: 1;
}

.pw-section-header .count {
    margin-left: auto;
    font-weight: 400;
    letter-spacing: 0;
    font-size: var(--pw-text-2xs);
    color: var(--rinch-color-placeholder);
}
"#;
