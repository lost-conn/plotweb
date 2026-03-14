use rinch::prelude::*;
use rinch_core::use_store;
use plotweb_common::LoginRequest;

use crate::api;
use crate::router;
use crate::store::{AppStore, Route};

#[component]
pub fn login_page() -> NodeHandle {
    let store = use_store::<AppStore>();
    let username = Signal::new(String::new());
    let password = Signal::new(String::new());
    let remember_me = Signal::new(false);
    let error = Signal::new(Option::<String>::None);
    let submitting = Signal::new(false);

    let on_submit = move || {
        if submitting.get() {
            return;
        }
        let u = username.get();
        let p = password.get();
        if u.is_empty() || p.is_empty() {
            error.set(Some("Please fill in all fields".into()));
            return;
        }
        submitting.set(true);
        error.set(None);
        wasm_bindgen_futures::spawn_local(async move {
            let req = LoginRequest {
                username: u,
                password: p,
                remember_me: remember_me.get(),
            };
            match api::post::<_, plotweb_common::User>("/api/auth/login", &req).await {
                Ok(user) => {
                    store.current_user.set(Some(user));
                    router::navigate(Route::Dashboard);
                }
                Err(e) => {
                    error.set(Some(e.message));
                }
            }
            submitting.set(false);
        });
    };

    let go_register = move || {
        router::navigate(Route::Register);
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

                Center {
                    img {
                        src: "/assets/logo.png",
                        alt: "PlotWeb",
                        style: "width: 72px; height: 72px;",
                    }
                }
                Space { h: "md" }
                Title { order: 2, "Welcome back" }
                Space { h: "xs" }
                Text { size: "sm", color: "dimmed", "Sign in to your PlotWeb account" }
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
                    placeholder: "Your username",
                    value_fn: move || username.get(),
                    oninput: move |v: String| username.set(v),
                }
                Space { h: "md" }
                PasswordInput {
                    label: "Password",
                    placeholder: "Your password",
                    value_fn: move || password.get(),
                    oninput: move |v: String| password.set(v),
                }
                Space { h: "md" }
                Checkbox {
                    label: "Remember me",
                    checked_fn: move || remember_me.get(),
                    onchange: move || remember_me.update(|v| *v = !*v),
                }
                Space { h: "xl" }
                Button {
                    full_width: true,
                    onclick: on_submit,
                    "Sign in"
                }
                Space { h: "md" }
                Center {
                    Button {
                        variant: "subtle",
                        onclick: go_register,
                        "Don't have an account? Register"
                    }
                }
            }
        }
    };
    page.set_attribute("data-onsubmit", &submit_id.0.to_string());
    page
}
