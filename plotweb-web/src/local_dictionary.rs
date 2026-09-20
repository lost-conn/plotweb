//! Local-first mirror of the account's custom spellcheck dictionary.
//!
//! The same shape as [`crate::local_user`] — a per-account copy kept in
//! [`rinch_storage`] and reconciled with the server — but far simpler, because the
//! thing being synced is a **set of strings** rather than a document with structure
//! and order. A set needs no CRDT: the merge is a union.
//!
//! # The merge is a union, always
//!
//! On entry the local list and the server list are unioned, and the result is written
//! to both sides. Nothing is ever dropped, on either side, for one reason: a word in
//! this list means *"an author looked at a squiggle and said that word is fine"*. The
//! cost of keeping a word too long is a typo that stays unflagged; the cost of
//! dropping one is that a name the author already dismissed starts being underlined
//! again on every page. Only one of those is worth a device's guess about which copy
//! is newer.
//!
//! Removal is therefore deliberately not expressible here. When the UI grows a
//! "remove word" affordance it will need a tombstone, and that is a different design
//! (see `plotweb_phase2_localfirst` for the same argument about books).
//!
//! # Where the words end up
//!
//! Three places, in this order: the [`AppStore::user_dictionary`] signal (so the UI
//! is right immediately), local storage (so a reload offline is right), and the
//! server (fire-and-forget; the local copy is what the next run reads). If a live
//! [`Speller`](crate::spell::Speller) has already been loaded it is updated too, so a
//! word added mid-session stops being flagged without reloading 79,000 others.

use std::collections::BTreeSet;

use crate::local_store::{backend, spawn};
use crate::store::AppStore;

/// The per-account cache key. Versioned like the bundled dictionary's, so a future
/// change of shape is a new key rather than a migration.
fn key(user_id: &str) -> String {
    format!("dict/user/v1/{user_id}/words")
}

/// Read this account's locally cached words.
async fn read_local(user_id: &str) -> Vec<String> {
    let Ok(store) = backend().await else {
        return Vec::new();
    };
    match store.get(&key(user_id)).await {
        Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Ok(None) => Vec::new(),
        Err(e) => {
            log::warn!("spell: could not read the local dictionary: {e}");
            Vec::new()
        }
    }
}

/// Replace this account's locally cached words.
async fn write_local(user_id: &str, words: &[String]) {
    let Ok(store) = backend().await else { return };
    let Ok(bytes) = serde_json::to_vec(words) else {
        return;
    };
    if let Err(e) = store.put(&key(user_id), &bytes).await {
        log::warn!("spell: could not save the local dictionary: {e}");
    }
}

/// Sorted union of two word lists.
fn union(a: &[String], b: &[String]) -> Vec<String> {
    let set: BTreeSet<&str> = a
        .iter()
        .chain(b.iter())
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .collect();
    set.into_iter().map(str::to_string).collect()
}

/// Publish `words` to the spellchecker and then to the signal.
///
/// That order is load-bearing. Rinch runs effects synchronously on `set`, and the
/// editor watches this signal to know its squiggles are stale
/// (`spell::plugin::user_words_changed`): it repaints from the plugin's cache.
/// Setting the signal first would repaint against a speller that had not been told
/// about the new word yet, and refill the cache with the very verdict the author
/// just overruled.
///
/// The words go to `spell::plugin::shared()` rather than straight into the
/// `Speller`, because on a cold start this list is ready long before the 860 KB
/// dictionary is; the plugin holds it until there is a speller to put it in.
fn install(words: Vec<String>, store: AppStore) {
    crate::spell::plugin::shared().set_user_words(words.clone());
    store.user_dictionary.set(words);
}

/// Bring this account's dictionary up: local copy first (so the UI and the speller
/// are right without the network), then the server's, unioned into both.
///
/// Safe to call more than once — everything it does is idempotent.
pub fn enter_user(user_id: String, store: AppStore) {
    spawn(async move {
        let local = read_local(&user_id).await;
        if !local.is_empty() {
            install(local.clone(), store);
        }

        // The pull is callback-based like the rest of `api`, so the local half above
        // is deliberately finished first: a device that is offline still ends up with
        // its own words rather than nothing.
        crate::api::get_user_dictionary(move |result| {
            let Ok(server) = result else {
                // Offline or signed out. The local copy already stands.
                return;
            };
            let merged = union(&local, &server.words);
            let local_had_more = merged.len() != server.words.len();
            install(merged.clone(), store);

            let uid = user_id.clone();
            let to_save = merged.clone();
            spawn(async move { write_local(&uid, &to_save).await });

            // Only push when this device actually knows something the server does
            // not — otherwise every app start writes the same list back.
            if local_had_more {
                crate::api::put_user_dictionary(&merged, |_| {});
            }
        });
    });
}

/// Add one word to this account's dictionary, everywhere it lives.
///
/// The server write is fire-and-forget: the local copy is what the next run reads,
/// and a failed push is retried implicitly by the union on next entry.
pub fn add_word(user_id: &str, word: &str, store: AppStore) {
    let word = word.trim().to_string();
    if word.is_empty() {
        return;
    }
    let mut words = store.user_dictionary.get();
    if words.iter().any(|w| w == &word) {
        return;
    }
    words.push(word.clone());
    words.sort();

    install(words.clone(), store);

    let user_id = user_id.to_string();
    let to_save = words.clone();
    spawn(async move { write_local(&user_id, &to_save).await });

    crate::api::put_user_dictionary(&words, |result| {
        if let Err(e) = result {
            log::warn!("spell: could not push the custom dictionary: {}", e.message);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_merges_sorts_and_drops_blanks() {
        let a = vec!["Elowen".to_string(), "  ".to_string(), "aeth".to_string()];
        let b = vec!["Corvane".to_string(), "Elowen".to_string()];
        assert_eq!(union(&a, &b), ["Corvane", "Elowen", "aeth"]);
    }

    #[test]
    fn union_never_drops_a_word_either_side_knows() {
        let local = vec!["OnlyHere".to_string()];
        let server = vec!["OnlyThere".to_string()];
        let merged = union(&local, &server);
        assert!(merged.contains(&"OnlyHere".to_string()));
        assert!(merged.contains(&"OnlyThere".to_string()));
    }

    #[test]
    fn union_of_identical_lists_changes_nothing() {
        let a = vec!["Elowen".to_string()];
        assert_eq!(union(&a, &a), a);
    }
}
