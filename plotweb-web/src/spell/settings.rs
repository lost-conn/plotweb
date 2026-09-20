//! The spellcheck switch, remembered **per device**.
//!
//! Deliberately not an account setting. Whether squiggles are wanted is a
//! property of where the author is writing rather than of who they are: the same
//! manuscript gets proofread on a laptop and drafted on a phone, and a setting
//! that followed the account would make one of those two wrong every time. It
//! lives in [`rinch_storage`] next to the dictionary cache, under `spell/v1/enabled`.
//!
//! Default **on**. An author who does not want a spellchecker discovers the
//! switch by being shown one; an author who does want one should not have to go
//! looking.

use crate::local_store::{backend, spawn};
use crate::store::AppStore;

/// The per-device key. Versioned like the dictionary cache's, so a future change
/// of shape is a new key rather than a migration.
const KEY: &str = "spell/v1/enabled";

/// Read the switch off this device and publish it — to the signal the UI renders
/// from, and to the plugin's shared state the editor reads.
///
/// A device that has never been asked (and one whose storage is unavailable)
/// keeps the default, so this only ever *turns the feature off*.
pub fn hydrate(store: AppStore) {
    spawn(async move {
        let Ok(backend) = backend().await else { return };
        let stored = match backend.get(KEY).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return,
            Err(e) => {
                log::warn!("spell: could not read the spellcheck switch: {e}");
                return;
            }
        };
        // Anything that is not an explicit "off" is on — a corrupt byte should not
        // silently disable a feature the author never turned off.
        let enabled = stored.as_slice() != b"0";
        store.spellcheck_enabled.set(enabled);
        super::plugin::shared().set_enabled(enabled);
    });
}

/// Remember the switch on this device. Fire-and-forget: the signal and the
/// plugin were already updated by the caller, and a failed write costs the
/// author one extra click next session.
pub fn persist(enabled: bool) {
    spawn(async move {
        let Ok(backend) = backend().await else { return };
        let value: &[u8] = if enabled { b"1" } else { b"0" };
        if let Err(e) = backend.put(KEY, value).await {
            log::warn!("spell: could not save the spellcheck switch: {e}");
        }
    });
}
