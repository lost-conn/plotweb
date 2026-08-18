//! Mirroring sync-originated writes into git (migration phase E).
//!
//! Under cutover the canonical document is the source of truth and git is the mirror.
//! A REST write keeps both in step by itself — it goes to git and is applied to the
//! canonical copy in the same request. A **sync** write does not: a syncing client
//! pushes an update straight into the canonical document and git never hears about it.
//!
//! Left alone that would quietly undo the reason for keeping git at all. Version
//! history, export and beta-reader views read git, so they would freeze at the moment
//! of cutover; and the flag would stop being a rollback, because flipping it back would
//! return the book to whatever git last saw rather than to current content. It would
//! also make the shadow pass permanently red for exactly the documents being used most.
//!
//! # Why it is debounced rather than immediate
//!
//! A client syncing while its author types pushes an update every second or two.
//! Committing each one would bury a book's history in noise — worse than today, where a
//! commit tracks a save. So a changed document is *marked*, and a background pass
//! writes it to git once it has been quiet for [`IDLE`], or in any case within
//! [`MAX_WAIT`] of the first change, so a long writing session still checkpoints.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use plotweb_common::UpdateChapterRequest;
use plotweb_crdt::BodyKind;

use crate::AppState;

/// How long a document must be quiet before it is written to git.
pub const IDLE: Duration = Duration::from_secs(30);
/// How long a continuously-edited document may go unwritten before it is checkpointed
/// anyway. Without this a long session never commits, which is the failure mode that
/// makes "git is the mirror" a promise rather than a fact.
pub const MAX_WAIT: Duration = Duration::from_secs(300);
/// How often the background pass looks for due documents.
const TICK: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
struct Pending {
    kind: BodyKind,
    first_marked: Instant,
    last_marked: Instant,
}

/// Documents whose canonical copy has moved and whose git mirror has not.
#[derive(Clone, Default)]
pub struct MirrorQueue {
    pending: Arc<StdMutex<HashMap<(String, String), Pending>>>,
}

/// Which body a `chapter:`/`note:` document id names, or `None` for anything else
/// (structure documents, which have no single git file to mirror into — see the module
/// docs of [`crate::sync`]).
pub fn kind_of_doc(doc_id: &str) -> Option<BodyKind> {
    if doc_id.starts_with("chapter:") {
        Some(BodyKind::Chapter)
    } else if doc_id.starts_with("note:") {
        Some(BodyKind::Note)
    } else {
        None
    }
}

impl MirrorQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Note that `doc_id` (in `book_id`) has changed and owes git a write.
    pub fn mark(&self, book_id: &str, doc_id: &str, kind: BodyKind) {
        let now = Instant::now();
        let mut pending = self.pending.lock().unwrap();
        pending
            .entry((book_id.to_string(), doc_id.to_string()))
            .and_modify(|p| p.last_marked = now)
            .or_insert(Pending {
                kind,
                first_marked: now,
                last_marked: now,
            });
    }

    pub fn len(&self) -> usize {
        self.pending.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Take everything due, leaving the rest. `idle`/`max_wait` are parameters so a
    /// test can demand everything immediately.
    fn take_due(&self, idle: Duration, max_wait: Duration) -> Vec<(String, String, BodyKind)> {
        let now = Instant::now();
        let mut pending = self.pending.lock().unwrap();
        let due: Vec<(String, String)> = pending
            .iter()
            .filter(|(_, p)| {
                now.duration_since(p.last_marked) >= idle
                    || now.duration_since(p.first_marked) >= max_wait
            })
            .map(|(key, _)| key.clone())
            .collect();
        due.into_iter()
            .filter_map(|key| {
                pending
                    .remove(&key)
                    .map(|p| (key.0.clone(), key.1.clone(), p.kind))
            })
            .collect()
    }
}

/// Write every due document's canonical content into git.
///
/// Returns how many documents were mirrored. Failures are logged and the document is
/// dropped from the queue rather than retried forever: the shadow pass reports the
/// resulting difference, which is a better signal than a queue silently growing.
pub async fn flush(state: &AppState, idle: Duration, max_wait: Duration) -> usize {
    let due = state.mirror.take_due(idle, max_wait);
    let mut written = 0;

    for (book_id, doc_id, kind) in due {
        // The canonical copy is read under the same per-document lock a sync write
        // takes, so a half-applied update is never what gets mirrored.
        let bytes = {
            let lock = state.doc_locks.for_doc(&doc_id);
            let _guard = lock.lock().await;
            match crate::sync::canonical_snapshot(&state.crdt_dir, &doc_id) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("[mirror] {doc_id}: store read failed: {e}");
                    continue;
                }
            }
        };

        let content = match plotweb_crdt::materialize_body(&bytes) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("[mirror] {doc_id}: could not materialize: {e}");
                continue;
            }
        };

        // Skip a document whose git copy already says this. A sync round can move the
        // canonical document without changing what it materializes to (a formatting
        // no-op, or an update the server already had), and committing that would put a
        // commit in the author's history that represents no change to their book.
        let current = match kind {
            BodyKind::Chapter => {
                let id = doc_id.trim_start_matches("chapter:");
                state.books.get_chapter(&book_id, id).await.map(|c| c.content)
            }
            BodyKind::Note => {
                let id = doc_id.trim_start_matches("note:");
                state.books.get_note(&book_id, id).await.map(|n| n.content)
            }
        };
        if current.as_deref().ok() == Some(content.as_str()) {
            continue;
        }

        let result = match kind {
            BodyKind::Chapter => {
                let id = doc_id.trim_start_matches("chapter:").to_string();
                state
                    .books
                    .update_chapter(
                        &book_id,
                        &id,
                        &UpdateChapterRequest {
                            title: None,
                            content: Some(content),
                        },
                    )
                    .await
            }
            BodyKind::Note => {
                let id = doc_id.trim_start_matches("note:").to_string();
                state
                    .books
                    .update_note(&book_id, &id, None, Some(&content), None)
                    .await
            }
        };

        match result {
            Ok(()) => {
                written += 1;
                println!("[mirror] {doc_id}: written to git");
            }
            Err(e) => eprintln!("[mirror] {doc_id}: git write failed: {e}"),
        }
    }
    written
}

/// The background pass. Cheap when idle: it holds no locks and touches nothing unless a
/// sync write has marked a document.
pub async fn run(state: AppState) {
    loop {
        tokio::time::sleep(TICK).await;
        flush(&state, IDLE, MAX_WAIT).await;
    }
}
