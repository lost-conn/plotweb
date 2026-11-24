pub mod login;
pub mod register;
pub mod forgot_password;
pub mod reset_password;
pub mod dashboard;
pub mod settings;
pub mod book;
pub mod editor_utils;
pub mod reader;
// Desktop-only vertical pagination for the reader (rinch-dom has no CSS columns).
#[cfg(not(target_arch = "wasm32"))]
pub mod reader_pager;
pub mod theme_preview;
pub mod editor_spike;
pub mod opfs_spike;
