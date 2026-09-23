//! The chrome-fade switch, remembered **per device**.
//!
//! Like [`crate::spell::settings`], deliberately not an account setting: whether
//! the sidebar/toolbar/topbar should get out of the way while typing is a property
//! of the screen the author is looking at (a cramped laptop wants the extra room;
//! a wide desktop monitor may not) rather than of who they are. Lives in
//! [`rinch_storage`] next to the spellcheck switch and the dictionary cache, under
//! `chrome/v1/fade`.
//!
//! Default **on** — the same "discover the switch by being shown one, not by going
//! looking for it" reasoning as the spellcheck switch.

use crate::local_store::{backend, spawn};
use crate::store::AppStore;

/// The per-device key. Versioned like the spellcheck switch's, so a future change
/// of shape is a new key rather than a migration.
const KEY: &str = "chrome/v1/fade";

/// Read the switch off this device and publish it to `store.chrome_fade_enabled`.
///
/// A device that has never been asked (and one whose storage is unavailable) keeps
/// the default, so this only ever *turns the feature off*.
pub fn hydrate(store: AppStore) {
    spawn(async move {
        let Ok(backend) = backend().await else { return };
        let stored = match backend.get(KEY).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return,
            Err(e) => {
                log::warn!("chrome: could not read the chrome-fade switch: {e}");
                return;
            }
        };
        // Anything that is not an explicit "off" is on — a corrupt byte should not
        // silently disable a feature the author never turned off.
        let enabled = stored.as_slice() != b"0";
        store.chrome_fade_enabled.set(enabled);
    });
}

/// Remember the switch on this device. Fire-and-forget: the signal was already
/// updated by the caller, and a failed write costs the author one extra click next
/// session.
pub fn persist(enabled: bool) {
    spawn(async move {
        let Ok(backend) = backend().await else { return };
        let value: &[u8] = if enabled { b"1" } else { b"0" };
        if let Err(e) = backend.put(KEY, value).await {
            log::warn!("chrome: could not save the chrome-fade switch: {e}");
        }
    });
}
