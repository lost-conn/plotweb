//! Client sync engine for the **structure** documents (`user:` and `book:`) —
//! sync engine slice 3 (`docs/sync-engine-design.md`).
//!
//! These two doc types are plain [`automerge::AutoCommit`]s owned by
//! [`crate::local_user`] / [`crate::local_book`], so the full Automerge sync protocol
//! is available here today. Chapter and note **bodies** live inside the editor's
//! collaboration session, whose handle does not yet expose the protocol — that is
//! slice 4, gated on an upstream rinch change.
//!
//! # Shape: callbacks, not futures
//!
//! An exchange is several HTTP round trips. `rinch_http` is callback-based on both
//! targets, and the local-first [`spawn`](crate::local_store::spawn) is a *single-poll*
//! driver natively (it exists for storage futures that resolve immediately, and would
//! drop a future that actually pends). So the exchange is written as an explicit
//! callback chain: each reply schedules the next round. Nothing here needs a runtime,
//! and nothing crosses a thread.
//!
//! # One cycle = one fresh `SyncState`
//!
//! Automerge's protocol assumes a live connection where the peer pushes. Our server is
//! stateless per request and cannot push, so a `SyncState` kept across polls would
//! convince us we are still converged and we would stop asking — and silently miss
//! whatever another device wrote. Each cycle therefore starts a **fresh** state and
//! throws it away at the end. (The server-side rationale is in
//! `plotweb-server/src/sync.rs`; the failure mode is real and was caught by a test.)
//!
//! # Still additive
//!
//! Every REST write remains in place; this only moves CRDT bytes. Off by default —
//! see [`enabled`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use automerge::sync::{Message as SyncMessage, State as SyncState, SyncDoc};

use crate::store::AppStore;

/// How long after a local change before pushing it (coalesces a burst of edits).
const NUDGE_DEBOUNCE_MS: u32 = 1_500;
/// Idle poll interval — how quickly another device's change shows up here.
const POLL_INTERVAL_MS: u32 = 20_000;
/// First backoff step after a server error; doubles up to [`MAX_BACKOFF_MS`].
const BASE_BACKOFF_MS: u32 = 5_000;
const MAX_BACKOFF_MS: u32 = 300_000;
/// Guard against a pathological exchange that never settles.
const MAX_ROUNDS: u32 = 12;

/// Which document a registration refers to, and therefore which local module owns the
/// CRDT and which URL syncs it.
#[derive(Clone, PartialEq, Eq)]
enum Doc {
    /// The signed-in account's index (`user:{id}`), synced at `/api/sync/user`.
    User(String),
    /// A book's structure (`book:{id}`).
    Book(String),
}

impl Doc {
    fn url(&self) -> String {
        match self {
            Doc::User(_) => "/api/sync/user".to_string(),
            Doc::Book(id) => format!("/api/books/{id}/sync/book:{id}"),
        }
    }

    fn label(&self) -> String {
        match self {
            Doc::User(id) => format!("user:{id}"),
            Doc::Book(id) => format!("book:{id}"),
        }
    }

    /// Run `f` against this doc's live CRDT, if it is still the open one.
    fn with_doc<R>(&self, f: impl FnOnce(&mut automerge::AutoCommit) -> R) -> Option<R> {
        match self {
            Doc::User(id) => crate::local_user::with_user_doc(id, f),
            Doc::Book(id) => crate::local_book::with_book_doc(id, f),
        }
    }

    /// Persist after a merge, then re-project into the render signals so a change
    /// that arrived from another device appears without a reload.
    fn persist_and_project(&self, store: &AppStore) {
        match self {
            Doc::User(id) => {
                crate::local_user::persist_user(id);
                crate::local_user::project_books(store.clone());
            }
            Doc::Book(id) => {
                crate::local_book::persist_book(id);
                crate::local_book::project(store.clone());
            }
        }
    }

    /// Whether this doc is still the one open in its module. A cycle for a book the
    /// user has navigated away from is abandoned rather than resurrected.
    fn still_open(&self) -> bool {
        match self {
            Doc::User(id) => crate::local_user::open_user_id().as_deref() == Some(id),
            Doc::Book(id) => crate::local_book::open_book_id().as_deref() == Some(id),
        }
    }
}

/// Per-document engine state. `Unauthed` is terminal until the next
/// [`register_user`] (i.e. the next sign-in) — a background loop must never
/// retry-storm a 401.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running,
    Unauthed,
}

struct Entry {
    doc: Doc,
    phase: Phase,
    /// Consecutive failures, for the backoff curve.
    failures: u32,
    /// A cycle was requested while one was running; run once more when it ends.
    again: bool,
    /// A poll/backoff timer is already armed (so we don't stack timers).
    armed: bool,
}

thread_local! {
    /// Registered documents, keyed by label. Small and short-lived: one `user:` entry
    /// plus the open book.
    static ENGINE: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());
    /// The app store, for re-projecting merged changes. Set on first registration.
    static STORE: RefCell<Option<AppStore>> = const { RefCell::new(None) };
}

/// Whether background sync runs at all.
///
/// Off unless explicitly switched on, per slice 3's rollout plan: native reads
/// `PLOTWEB_SYNC=1`, web reads `localStorage["plotweb_sync"] == "1"`. When off, every
/// entry point here is a no-op and the app behaves exactly as it did before.
#[cfg(not(target_arch = "wasm32"))]
pub fn enabled() -> bool {
    std::env::var("PLOTWEB_SYNC").map(|v| v == "1").unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
pub fn enabled() -> bool {
    crate::platform::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("plotweb_sync").ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Register the signed-in account's `user:` doc and sync it now.
///
/// Also clears any previous `Unauthed` phase: a fresh sign-in is exactly the event
/// that makes syncing worth attempting again.
pub fn register_user(user_id: &str, store: AppStore) {
    register(Doc::User(user_id.to_string()), store);
}

/// Register the open book's `book:` doc and sync it now.
pub fn register_book(book_id: &str, store: AppStore) {
    register(Doc::Book(book_id.to_string()), store);
}

fn register(doc: Doc, store: AppStore) {
    if !enabled() {
        return;
    }
    STORE.with(|s| *s.borrow_mut() = Some(store));
    let label = doc.label();
    ENGINE.with(|e| {
        e.borrow_mut().insert(
            label.clone(),
            Entry {
                doc,
                phase: Phase::Idle,
                failures: 0,
                again: false,
                armed: false,
            },
        );
    });
    start_cycle(&label);
}

/// A local change landed — push it soon (debounced, so a burst of edits is one push).
pub fn nudge(label_owner: &str, is_book: bool) {
    if !enabled() {
        return;
    }
    let label = if is_book {
        format!("book:{label_owner}")
    } else {
        format!("user:{label_owner}")
    };
    arm_timer(&label, NUDGE_DEBOUNCE_MS);
}

// ── The cycle ────────────────────────────────────────────────────────────────

/// Begin an exchange for `label`, unless one is already running (in which case it is
/// flagged to run again afterwards) or we are signed out.
fn start_cycle(label: &str) {
    let ready = ENGINE.with(|e| {
        let mut map = e.borrow_mut();
        let Some(entry) = map.get_mut(label) else {
            return false;
        };
        match entry.phase {
            Phase::Unauthed => false,
            Phase::Running => {
                entry.again = true;
                false
            }
            Phase::Idle => {
                entry.phase = Phase::Running;
                true
            }
        }
    });
    if ready {
        round(label.to_string(), Rc::new(RefCell::new(SyncState::new())), 0);
    }
}

/// One request/response of an exchange, recursing until we have nothing left to send.
fn round(label: String, state: Rc<RefCell<SyncState>>, n: u32) {
    let Some(doc) = doc_of(&label) else { return };
    if !doc.still_open() {
        finish(&label, true);
        return;
    }
    if n >= MAX_ROUNDS {
        log::warn!("sync {label}: exchange did not settle in {MAX_ROUNDS} rounds");
        finish(&label, false);
        return;
    }

    // Termination is ours: the server is stateless and always replies.
    let outgoing = doc.with_doc(|d| d.sync().generate_sync_message(&mut state.borrow_mut()));
    let Some(Some(msg)) = outgoing else {
        finish(&label, true);
        return;
    };

    let url = doc.url();
    crate::api::post_bytes(&url, msg.encode(), move |result| match result {
        Ok(reply) => {
            if reply.is_empty() {
                finish(&label, true);
                return;
            }
            let Some(doc) = doc_of(&label) else { return };
            match SyncMessage::decode(&reply) {
                Ok(message) => {
                    let integrated = doc.with_doc(|d| {
                        d.sync()
                            .receive_sync_message(&mut state.borrow_mut(), message)
                            .is_ok()
                    });
                    match integrated {
                        Some(true) => {
                            STORE.with(|s| {
                                if let Some(store) = s.borrow().as_ref() {
                                    doc.persist_and_project(store);
                                }
                            });
                            round(label, state, n + 1);
                        }
                        // The doc closed under us, or the message was rejected.
                        _ => finish(&label, false),
                    }
                }
                Err(e) => {
                    log::warn!("sync {label}: undecodable reply: {e}");
                    finish(&label, false);
                }
            }
        }
        // Signed out: stop, quietly, until the next sign-in re-registers us.
        Err(e) if e.status == 401 => {
            log::info!("sync {label}: not signed in; pausing sync");
            ENGINE.with(|eng| {
                if let Some(entry) = eng.borrow_mut().get_mut(&label) {
                    entry.phase = Phase::Unauthed;
                    entry.again = false;
                }
            });
        }
        // 403/404 — not ours, or gone. Retrying can't help; drop the registration.
        Err(e) if e.status == 403 || e.status == 404 => {
            log::warn!("sync {label}: rejected ({}); unregistering", e.status);
            ENGINE.with(|eng| {
                eng.borrow_mut().remove(&label);
            });
        }
        // Offline (status 0) or a server error: back off and try later.
        Err(e) => {
            log::debug!("sync {label}: {} {}", e.status, e.message);
            finish(&label, false);
        }
    });
}

/// End an exchange: reset or advance the backoff, then arm the next attempt.
fn finish(label: &str, success: bool) {
    let next = ENGINE.with(|e| {
        let mut map = e.borrow_mut();
        let Some(entry) = map.get_mut(label) else {
            return None;
        };
        entry.phase = Phase::Idle;
        if success {
            entry.failures = 0;
        } else {
            entry.failures = entry.failures.saturating_add(1);
        }
        let run_again = std::mem::take(&mut entry.again);
        Some(if run_again {
            0
        } else if success {
            POLL_INTERVAL_MS
        } else {
            backoff(entry.failures)
        })
    });
    match next {
        Some(0) => start_cycle(label),
        Some(delay) => arm_timer(label, delay),
        None => {}
    }
}

/// Exponential backoff with a ceiling: 5s, 10s, 20s … capped at 5 min.
fn backoff(failures: u32) -> u32 {
    BASE_BACKOFF_MS
        .saturating_mul(1u32 << failures.min(6))
        .min(MAX_BACKOFF_MS)
}

/// Arm a one-shot timer to start a cycle, unless one is already pending. Uses
/// rinch's cross-platform timer (`window.setTimeout` on web, the shared timer thread
/// natively), so this works in both shells.
fn arm_timer(label: &str, delay_ms: u32) {
    let already = ENGINE.with(|e| {
        let mut map = e.borrow_mut();
        match map.get_mut(label) {
            Some(entry) if !entry.armed => {
                entry.armed = true;
                false
            }
            Some(_) => true,
            None => true,
        }
    });
    if already {
        return;
    }
    let label = label.to_string();
    rinch_core::set_timeout(delay_ms, move || {
        ENGINE.with(|e| {
            if let Some(entry) = e.borrow_mut().get_mut(&label) {
                entry.armed = false;
            }
        });
        start_cycle(&label);
    });
}

fn doc_of(label: &str) -> Option<Doc> {
    ENGINE.with(|e| e.borrow().get(label).map(|entry| entry.doc.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff(0), BASE_BACKOFF_MS);
        assert_eq!(backoff(1), BASE_BACKOFF_MS * 2);
        assert_eq!(backoff(2), BASE_BACKOFF_MS * 4);
        assert_eq!(backoff(20), MAX_BACKOFF_MS, "backoff is capped");
    }

    #[test]
    fn urls_are_book_scoped() {
        assert_eq!(Doc::User("u1".into()).url(), "/api/sync/user");
        assert_eq!(
            Doc::Book("b1".into()).url(),
            "/api/books/b1/sync/book:b1",
            "the book doc syncs on its own book's route"
        );
    }
}

#[cfg(test)]
mod seam_canary {
    /// The `rinch` pin must carry the Automerge sync-protocol seam on `EditorHandle`
    /// (rinch PR #182). Slice 4 (chapter/note body sync) is built on these three
    /// methods and they exist on no released rev, so a pin that slips back to
    /// upstream `main` must fail here — loudly and at compile time — rather than
    /// deep inside the body-sync work.
    #[test]
    fn the_editor_handle_exposes_the_sync_protocol() {
        use crate::rinch_backend::EditorHandle;
        let _generate: fn(
            &EditorHandle,
            &mut rinch_editor_collab::SyncState,
        ) -> Option<rinch_editor_collab::SyncMessage> =
            EditorHandle::collab_generate_sync_message;
        let _receive: fn(
            &EditorHandle,
            &mut rinch_editor_collab::SyncState,
            rinch_editor_collab::SyncMessage,
        ) -> bool = EditorHandle::collab_receive_sync_message;
        let _heads: fn(&EditorHandle) -> Option<Vec<rinch_editor_collab::ChangeHash>> =
            EditorHandle::collab_heads;
    }
}
