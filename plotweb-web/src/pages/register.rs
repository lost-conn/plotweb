use rinch::prelude::*;
use rinch_core::use_store;
use plotweb_common::RegisterRequest;

use crate::api;
use crate::store::{AppStore, Route};

#[component]
pub fn register_page() -> NodeHandle {
    let store = use_store::<AppStore>();
    let username = Signal::new(String::new());
    let email = Signal::new(String::new());
    let password = Signal::new(String::new());
    let confirm_password = Signal::new(String::new());
    let error = Signal::new(Option::<String>::None);
    let submitting = Signal::new(false);

    let on_submit = move || {
        if submitting.get() {
            return;
        }
        let u = username.get();
        let e = email.get();
        let p = password.get();
        let cp = confirm_password.get();

        if u.is_empty() || e.is_empty() || p.is_empty() {
            error.set(Some("Please fill in all fields".into()));
            return;
        }
        if p != cp {
            error.set(Some("Passwords do not match".into()));
            return;
        }

        submitting.set(true);
        error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = RegisterRequest {
                username: u,
                email: e,
                password: p,
            };
            match api::post::<_, plotweb_common::User>("/api/auth/register", &req).await {
                Ok(user) => {
                    store.current_user.set(Some(user));
                    store.current_route.set(Route::Dashboard);
                }
                Err(e) => {
                    error.set(Some(e.message));
                }
            }
            submitting.set(false);
        });
    };

    let go_login = move || {
        store.current_route.set(Route::Login);
    };

    let submit_id = __scope.register_handler(on_submit);

    let page = rsx! {
        div {
            class: "auth-page",
            Paper {
                shadow: "md",
                p: "xl",
                radius: "md",
                w: "400px",

                Title { order: 2, "Create account" }
                Space { h: "xs" }
                Text { size: "sm", color: "dimmed", "Start writing with PlotWeb" }
                Space { h: "lg" }

                if error.get().is_some() {
                    Alert {
                        color: "red",
                        title: "Error",
                        {error.get().unwrap_or_default()}
                    }
                    Space { h: "md" }
                }

                TextInput {
                    label: "Username",
                    placeholder: "Choose a username",
                    value_fn: move || username.get(),
                    oninput: move |v: String| username.set(v),
                }
                Space { h: "md" }
                TextInput {
                    label: "Email",
                    placeholder: "your@email.com",
                    value_fn: move || email.get(),
                    oninput: move |v: String| email.set(v),
                }
                Space { h: "md" }
                PasswordInput {
                    label: "Password",
                    placeholder: "Choose a password",
                    value_fn: move || password.get(),
                    oninput: move |v: String| password.set(v),
                }
                Space { h: "md" }
                PasswordInput {
                    label: "Confirm Password",
                    placeholder: "Repeat your password",
                    value_fn: move || confirm_password.get(),
                    oninput: move |v: String| confirm_password.set(v),
                }
                Space { h: "xl" }
                Button {
                    full_width: true,
                    onclick: on_submit,
                    "Create account"
                }
                Space { h: "md" }
                Center {
                    Button {
                        variant: "subtle",
                        onclick: go_login,
                        "Already have an account? Sign in"
                    }
                }
            }
        }
    };
    page.set_attribute("data-onsubmit", &submit_id.0.to_string());
    page
}
