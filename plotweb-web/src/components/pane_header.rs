use rinch::prelude::*;

/// Top of any main-pane surface (`.pw-pane-header` in the mockup): a
/// display-face title, a quiet subtitle carrying count/state, and a
/// hairline beneath. `children` is the right-aligned actions slot
/// (`margin-left: auto`) — buttons, a `⋯` menu, whatever the pane needs.
///
/// Built generically so all seven panes can adopt it; this stage only
/// wires it into the chapters pane (see `pages/book/panes/chapters.rs`).
#[component]
pub fn PaneHeader(
    title: String,
    subtitle: String,
    style: String,
    children: &[NodeHandle],
) -> NodeHandle {
    let header = rsx! {
        div {
            class: "pw-pane-header",
            style: style,
            h4 { {title.clone()} }
            if !subtitle.is_empty() {
                span { class: "sub", {subtitle.clone()} }
            }
        }
    };
    if !children.is_empty() {
        let actions = rsx! { span { class: "actions" } };
        for child in children {
            actions.append_child(child);
        }
        header.append_child(&actions);
    }
    header
}

pub const PANE_HEADER_CSS: &str = r#"
.pw-pane-header {
    display: flex;
    align-items: baseline;
    gap: var(--pw-space-sm);
    padding-bottom: var(--pw-space-sm);
    margin-bottom: var(--pw-space-lg);
    border-bottom: 1px solid var(--pw-hairline);
}

.pw-pane-header h4 {
    font-family: var(--pw-font-display);
    font-weight: 400;
    font-size: var(--pw-text-lg);
}

.pw-pane-header .sub {
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
}

.pw-pane-header .actions {
    margin-left: auto;
    display: flex;
    gap: var(--pw-space-xs);
    align-items: center;
}
"#;
