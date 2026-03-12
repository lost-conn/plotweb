pub mod login;
pub mod register;
pub mod dashboard;
pub mod book;
pub mod editor;

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
        Route::Editor(book_id, chapter_id) => editor::editor_page(__scope, book_id, chapter_id),
    }
}
