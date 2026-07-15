//! Render-backend shim.
//!
//! The rich-text editor primitives (and the app mount, in `main.rs`) come from
//! `rinch-web` (browser DOM) on the web target and from `rinch` (the native
//! winit/wgpu shell) on desktop. App code imports the editor types from here so it
//! stays backend-agnostic; only this module and the entry point in `main.rs` know
//! which backend is active.

#[cfg(target_arch = "wasm32")]
pub use rinch_web::{Editor, EditorHandle, create_editor};

#[cfg(not(target_arch = "wasm32"))]
pub use rinch::prelude::{Editor, EditorHandle, create_editor};
