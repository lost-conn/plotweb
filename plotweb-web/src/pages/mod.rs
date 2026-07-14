pub mod login;
pub mod register;
pub mod forgot_password;
pub mod reset_password;
pub mod dashboard;
pub mod book;
pub mod editor_utils;
pub mod reader;
pub mod theme_preview;
pub mod editor_spike;
pub mod opfs_spike;
pub mod sync_spike;

use rinch::prelude::*;
use rinch_core::use_store;
use crate::store::{AppStore, Route};

pub fn route_content(__scope: &mut RenderScope) -> NodeHandle {
    let store = use_store::<AppStore>();
    let route = store.current_route.get();

    match route {
        Route::Login => login::login_page(__scope),
        Route::Register => register::register_page(__scope),
        Route::ForgotPassword => forgot_password::forgot_password_page(__scope),
        Route::ResetPassword(token) => reset_password::reset_password_page(__scope, token),
        Route::Dashboard => dashboard::dashboard_page(__scope),
        Route::Book(id) => book::book_page(__scope, id),
        Route::Reader(token) => reader::reader_page(__scope, token),
        Route::ReaderPreview(book_id) => reader::reader_preview_page(__scope, book_id),
        Route::ThemePreview => theme_preview::theme_preview_page(__scope),
        Route::EditorSpike => editor_spike::editor_spike_page(__scope),
        Route::OpfsSpike => opfs_spike::opfs_spike_page(__scope),
        Route::SyncSpike => sync_spike::sync_spike_page(__scope),
    }
}
