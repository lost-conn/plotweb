use rinch::prelude::*;

/// Tier 2 of the book page's overlay tiers (see `design/01-language.html#overlays`):
/// a small, centred confirm, reserved for destructive or irreversible actions —
/// restore a version, delete a chapter. Everything else either edits in place
/// (Tier 1) or opens a [`crate::components::sheet::Sheet`] (Tier 3).
///
/// Wraps rinch's `Modal` so no call site re-declares its own
/// `Space + Group + Cancel/Confirm` footer — the nine modals this replaces each
/// did, which is most of the ~750 lines this stage removes. `children` is the
/// body only; the footer is a fixed shape.
///
/// `opened_fn` is forwarded straight through to the inner `Modal`'s own
/// `opened_fn` — a reactive `_fn` prop on a plain `#[component]` re-renders the
/// *whole* component on every signal change (rinch wraps it in
/// `reactive_component_dom`), which would tear down and rebuild the `Modal`
/// subtree on every open/close. Naming this field `opened_fn` makes the `rsx!`
/// macro auto-wrap the caller's closure as `Some(Rc::new(..))` without
/// triggering that reactive-component path (only *non*-`_fn`, non-`on*` props
/// do), so it reaches `Modal` unevaluated and `Modal`'s own surgical
/// class-toggle effect is what actually runs on toggle.
///
/// `danger` swaps the confirm button to red, for "this cannot be undone" actions.
/// `confirm_label` defaults to "Confirm" when empty so a call site can't forget it
/// silently (an empty rinch `Button` label would otherwise render a blank button).
#[component]
pub fn Dialog(
    opened_fn: Option<std::rc::Rc<dyn Fn() -> bool>>,
    onclose: Callback,
    title: String,
    danger: bool,
    confirm_label: String,
    cancel_label: String,
    onconfirm: Callback,
    children: &[NodeHandle],
) -> NodeHandle {
    let confirm_text = if confirm_label.is_empty() { "Confirm".to_string() } else { confirm_label };
    let cancel_text = if cancel_label.is_empty() { "Cancel".to_string() } else { cancel_label };
    let confirm_color = if danger { "red" } else { "" };

    let body = rsx! { div { class: "pw-dialog-body" } };
    for child in children {
        body.append_child(child);
    }

    rsx! {
        Modal {
            // `opened_fn` is already `Option<Rc<dyn Fn() -> bool>>` — passing it
            // as-is would hit the macro's `_fn`-suffix rule a second time and
            // double-wrap it. Re-closuring it lets the macro wrap correctly
            // while still forwarding an unevaluated getter into `Modal`, so its
            // own class-toggle effect (not a rebuild of this component) is what
            // reacts to the signal.
            opened_fn: {
                let opened_fn = opened_fn.clone();
                move || opened_fn.as_ref().is_some_and(|f| f())
            },
            onclose: {
                let onclose = onclose.clone();
                move || onclose.invoke()
            },
            title: title.clone(),
            centered: true,
            radius: "md",
            size: "sm",

            {body}
            Space { h: "lg" }
            Group {
                justify: "flex-end",
                Button {
                    variant: "subtle",
                    onclick: move || onclose.invoke(),
                    {cancel_text.clone()}
                }
                Button {
                    color: confirm_color,
                    onclick: move || onconfirm.invoke(),
                    {confirm_text.clone()}
                }
            }
        }
    }
}

pub const DIALOG_CSS: &str = r#"
.pw-dialog-body {
    font-size: var(--pw-text-sm);
    color: var(--rinch-color-text);
    line-height: var(--pw-lh-ui);
}

.pw-dialog-body .pw-dialog-warning {
    display: block;
    margin-top: var(--pw-space-xs);
    font-size: var(--pw-text-xs);
    color: var(--rinch-color-dimmed);
}

.rinch-modal {
    box-shadow: var(--pw-shadow-3);
    border-radius: var(--pw-radius-md);
}
"#;
