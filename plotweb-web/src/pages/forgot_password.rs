use rinch::prelude::*;
use rinch_core::use_store;
use plotweb_common::ForgotPasswordRequest;

use crate::api;
use crate::components::auth_shell::AuthShell;
use crate::router;
use crate::store::{AppStore, Route};

#[component]
pub fn forgot_password_page() -> NodeHandle {
    let _store = use_store::<AppStore>();
    let email = Signal::new(String::new());
    let error = Signal::new(Option::<String>::None);
    let submitting = Signal::new(false);
    // Once submitted we always show the same neutral confirmation, regardless of
    // whether the address was registered — mirrors the server's non-enumerating
    // response.
    let sent = Signal::new(false);

    let on_submit = move || {
        if submitting.get() || sent.get() {
            return;
        }
        let e = email.get();
        if e.trim().is_empty() {
            error.set(Some("Please enter your email".into()));
            return;
        }
        submitting.set(true);
        error.set(None);
        let req = ForgotPasswordRequest { email: e };
        api::post::<_, serde_json::Value>("/api/auth/forgot-password", &req, move |result| {
            match result {
                Ok(_) => sent.set(true),
                Err(e) => error.set(Some(e.message)),
            }
            submitting.set(false);
        });
    };

    let go_login = move || {
        router::navigate(Route::Login);
    };

    let submit_id = __scope.register_handler(on_submit);

    let page = rsx! {
        div {
            class: "auth-page",
            AuthShell {
                title: "Reset your password",
                subtitle: "We'll email you a link to choose a new password",

                if sent.get() {
                    Alert {
                        color: "teal",
                        title: "Check your email",
                        "If an account exists for that address, a password reset link is on its way. The link expires in 1 hour."
                    }
                    Space { h: "md" }
                    Button {
                        full_width: true,
                        onclick: go_login,
                        "Go to sign in"
                    }
                } else {
                    if error.get().is_some() {
                        Alert {
                            color: "red",
                            title: "Error",
                            {move || error.get().unwrap_or_default()}
                        }
                        Space { h: "md" }
                    }

                    TextInput {
                        label: "Email",
                        placeholder: "your@email.com",
                        value_fn: move || email.get(),
                        oninput: move |v: String| email.set(v),
                    }
                    Space { h: "xl" }
                    Button {
                        full_width: true,
                        onclick: on_submit,
                        "Send reset link"
                    }
                    Space { h: "md" }
                    div {
                        class: "auth-foot",
                        Button {
                            variant: "subtle",
                            onclick: go_login,
                            "Back to sign in"
                        }
                    }
                }
            }
        }
    };
    page.set_attribute("data-onsubmit", &submit_id.0.to_string());
    page
}
