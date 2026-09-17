use rinch::prelude::*;

/// The 180×252 book object on the dashboard shelf. Per the `#dash` mockup
/// (`design/02-screens.html`), a book with no cover art is no longer a blank
/// rectangle with a teal stripe — it gets a *generated plate*: title in the
/// display face, an inset hairline frame, a rule, and the author line pinned
/// to the foot. A book with `cover_image: Some(..)` shows that image instead,
/// filling the jacket with `object-fit: cover`.
///
/// `shared` swaps the spine (and plate frame accent) from teal to violet for
/// the "Shared with me" shelf.
///
/// `children` is the hover-revealed actions slot in the top-right corner (the
/// `⋯` button) — optional, so the "new book" tile and other non-actionable
/// uses of the jacket shape don't have to pass an empty slot.
#[component]
pub fn BookJacket(
    title: String,
    author: String,
    cover_image: Option<String>,
    shared: bool,
    onclick: Callback,
    children: &[NodeHandle],
) -> NodeHandle {
    let has_cover = cover_image.is_some();
    let cover_url = cover_image.unwrap_or_default();

    let mut classes = vec!["bk-jacket"];
    if shared {
        classes.push("bk-jacket--shared");
    }
    let class_str = classes.join(" ");

    // Built separately (rather than nested directly in the rsx! below) so the
    // actions slot's children — passed in as an already-rendered `&[NodeHandle]`,
    // same as AuthShell's card — can be appended onto it before it is spliced
    // into the jacket as a plain node.
    let actions = rsx! { div { class: "bk-jacket-actions" } };
    for child in children {
        actions.append_child(child);
    }

    rsx! {
        div {
            class: class_str,
            onclick: move || onclick.invoke(),

            if has_cover {
                img { class: "bk-cover", src: cover_url.clone() }
            } else {
                div { class: "bk-plate",
                    div { class: "bk-plate-title", {title.clone()} }
                    div { class: "bk-plate-rule" }
                    div { class: "bk-plate-author", {author.clone()} }
                }
            }

            div { class: "bk-spine" }

            {actions}
        }
    }
}

pub const BOOK_JACKET_CSS: &str = r#"
/* The hairline is what makes this read as an object in dark mode. The plate
   gradient runs surface → deep, and deep is *darker* than the page, so the
   lower two-thirds of a jacket recedes into the background; the resting
   shadow cannot rescue it either, since a 0.4-alpha black shadow is invisible
   against #1C1917. In light mode the shadow does the work and the border is
   merely a crisper edge. */
.bk-jacket {
    position: relative;
    width: 180px;
    height: 252px;
    border: 1px solid var(--rinch-color-border);
    border-radius: 2px var(--pw-radius-md) var(--pw-radius-md) 2px;
    overflow: hidden;
    background: var(--rinch-color-surface);
    box-shadow: var(--pw-shadow-1);
    cursor: pointer;
    transition: transform var(--pw-dur) var(--pw-ease), box-shadow var(--pw-dur) var(--pw-ease);
}

.bk-jacket:hover {
    transform: translateY(-5px);
    box-shadow: var(--pw-shadow-3);
}

.bk-spine {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 5px;
    background: linear-gradient(180deg, var(--rinch-color-teal-6), var(--rinch-color-teal-9));
    box-shadow: inset -1px 0 2px rgba(0, 0, 0, 0.35);
}

.bk-jacket--shared .bk-spine {
    background: linear-gradient(180deg, var(--rinch-color-violet-6), var(--rinch-color-violet-9));
}

/* Generated jacket for books with no cover art. */
.bk-plate {
    position: absolute;
    inset: 0;
    padding: 30px var(--pw-space-md) 26px var(--pw-space-lg);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    background: radial-gradient(120% 80% at 50% 0%, var(--rinch-color-surface), var(--pw-color-deep));
}

.bk-plate::before {
    content: '';
    position: absolute;
    inset: 9px 9px 9px 13px;
    border: 1px solid var(--pw-hairline);
    border-radius: 2px;
    pointer-events: none;
}

.bk-plate-title {
    font-family: var(--pw-font-display);
    font-size: 18px;
    line-height: var(--pw-lh-tight);
}

.bk-plate-rule {
    width: 26px;
    height: 1px;
    background: var(--rinch-color-border);
    margin: var(--pw-space-sm) 0 0;
}

.bk-plate-author {
    position: absolute;
    left: 22px;
    right: 16px;
    bottom: var(--pw-space-lg);
    font-size: var(--pw-text-2xs);
    letter-spacing: .14em;
    text-transform: uppercase;
    color: var(--rinch-color-placeholder);
}

.bk-cover {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
}

.bk-jacket-actions {
    position: absolute;
    top: var(--pw-space-xs);
    right: var(--pw-space-xs);
    opacity: 0;
    transition: opacity var(--pw-dur-fast) var(--pw-ease);
}

.bk-jacket:hover .bk-jacket-actions {
    opacity: 1;
}
.bk-jacket-actions:focus-within {
    opacity: 1;
}
"#;
