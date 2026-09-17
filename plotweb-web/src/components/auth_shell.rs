use rinch::prelude::*;

/// The shared frame for the four auth pages (login, register, forgot-password,
/// reset-password) — logo, title, subtitle, then whatever page-specific fields
/// and actions the caller nests inside.
///
/// Was a `Paper { shadow: "md", p: "xl", radius: "md", w: "400px" }` block copied
/// byte-for-byte into all four pages. Per the `#auth` mockup (`design/02-screens.html`)
/// the card now sits on a radial wash with nothing behind it to separate from, so it
/// carries no border or shadow of its own — see `.auth-page` in `app_shell.rs`.
#[component]
pub fn AuthShell(title: String, subtitle: String, children: &[NodeHandle]) -> NodeHandle {
    let page = rsx! {
        div {
            class: "auth-card",

            div {
                class: "auth-mark",
                img {
                    src: crate::platform::asset_src("/assets/logo.png"),
                    alt: "PlotWeb",
                    style: "width: 34px; height: 34px;",
                }
                Title { order: 3, {title} }
                if !subtitle.is_empty() {
                    Text { size: "xs", color: "dimmed", {subtitle.clone()} }
                }
            }
        }
    };
    for child in children {
        page.append_child(child);
    }
    page
}
