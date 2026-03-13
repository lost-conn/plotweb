pub mod login;
pub mod register;
pub mod dashboard;
pub mod book;
pub mod editor_utils;
pub mod theme_preview;

use rinch::prelude::*;
use rinch_core::use_store;
use crate::store::{AppStore, Route};

pub fn route_content(__scope: &mut RenderScope) -> NodeHandle {
    let store = use_store::<AppStore>();
    let route = store.current_route.get();

    match route {
        Route::Login => login::login_page(__scope),
        Route::Register => register::register_page(__scope),
        Route::Dashboard => dashboard::dashboard_page(__scope),
        Route::Book(id) => book::book_page(__scope, id),
        Route::ThemePreview => theme_preview::theme_preview_page(__scope),
    }
}
