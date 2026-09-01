//! Reaching the copies a rebuild set aside.
//!
//! When `reconcile --prefer git` rebuilds a canonical document, the document it
//! replaces is copied to `_quarantine/{doc_id}/e{epoch}/snapshot` (see
//! [`crate::sync::replace_canonical_from_git`]). That copy is the only server-side
//! record of what every connected device was syncing against at the moment their
//! history was orphaned.
//!
//! Keeping bytes nobody can read is only marginally better than not keeping them, and
//! the 2026-08-29 recovery was carved out of a browser's IndexedDB by hand because
//! nothing on the server had the text. This is the read side: list what is held, and
//! materialize one back into the text it was.
//!
//! Read-only, lock-free, and safe against production — like the audit and the shadow
//! report, and unlike the reconcile that produces its input.
//!
//! # Two shapes, and why `show` sometimes cannot help
//!
//! A **client-owned** body holds the editor's own yrs document, which carries no
//! `meta.format` tag because it was never a server-made projection —
//! [`plotweb_crdt::materialize_body`] refuses it, and those are exactly the documents a
//! rebuild retires. A document the *server* projected reads straight back as text.
//!
//! So [`read`] always works and [`materialize`] sometimes does not, which is why the
//! error says so rather than reporting absence: the bytes are there and can be salvaged
//! by hand (`cargo run -p plotweb-crdt --example salvage_deep -- <dir>`), and a copy you
//! cannot pretty-print is still a copy you have.

use std::path::{Path, PathBuf};

use plotweb_crdt::BodyKind;
use rinch_storage::{FsStore, Store};

/// One quarantined copy: which document, which epoch retired it, how big it is.
#[derive(Debug, Clone)]
pub struct Entry {
    pub doc_id: String,
    pub epoch: u64,
    pub bytes: usize,
}

/// Every quarantined copy the store holds, oldest document first.
pub fn list(crdt_dir: &Path) -> Result<Vec<Entry>, String> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| format!("open {}: {e}", crdt_dir.display()))?;
    let keys = block_on(store.list("_quarantine/")).map_err(|e| e.to_string())?;

    let mut found = Vec::new();
    for key in keys {
        // `_quarantine/{doc_id}/e{n}/snapshot`
        let Some(rest) = key.strip_prefix("_quarantine/") else {
            continue;
        };
        let Some(base) = rest.strip_suffix("/snapshot") else {
            continue;
        };
        let Some((doc_id, epoch)) = base.rsplit_once('/') else {
            continue;
        };
        let Some(epoch) = epoch.strip_prefix('e').and_then(|n| n.parse::<u64>().ok()) else {
            continue;
        };
        let bytes = block_on(store.get(&key))
            .map_err(|e| e.to_string())?
            .map(|b| b.len())
            .unwrap_or(0);
        found.push(Entry {
            doc_id: doc_id.to_string(),
            epoch,
            bytes,
        });
    }
    found.sort_by(|a, b| a.doc_id.cmp(&b.doc_id).then(a.epoch.cmp(&b.epoch)));
    Ok(found)
}

/// The raw bytes of one quarantined copy.
pub fn read(crdt_dir: &Path, doc_id: &str, epoch: u64) -> Result<Option<Vec<u8>>, String> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| format!("open {}: {e}", crdt_dir.display()))?;
    block_on(store.get(&format!("_quarantine/{doc_id}/e{epoch}/snapshot")))
        .map_err(|e| e.to_string())
}

/// Materialize a quarantined **body** copy back into its `DocNode` JSON.
///
/// `Ok(None)` when there is no such copy; `Err` when the bytes are there but will not
/// project — which is worth saying out loud rather than reporting as absence, because
/// the bytes can still be salvaged by hand (`plotweb-crdt`'s `salvage_deep` example).
pub fn materialize(crdt_dir: &Path, doc_id: &str, epoch: u64) -> Result<Option<String>, String> {
    let Some(bytes) = read(crdt_dir, doc_id, epoch)? else {
        return Ok(None);
    };
    let kind = if doc_id.starts_with("note:") {
        BodyKind::Note
    } else {
        BodyKind::Chapter
    };
    plotweb_crdt::materialize_body(&bytes)
        .map(Some)
        .map_err(|e| format!("{doc_id} e{epoch} ({kind:?}) did not project: {e}"))
}

/// `plotweb-server quarantine list` / `quarantine show <doc_id> <epoch>`.
pub async fn run(sub: Option<&str>, rest: &[String]) {
    let crdt_dir =
        PathBuf::from(std::env::var("PLOTWEB_CRDT_DIR").unwrap_or_else(|_| "data/crdt".into()));

    match sub {
        Some("list") | None => match list(&crdt_dir) {
            Ok(entries) if entries.is_empty() => {
                println!("No quarantined copies. A rebuild sets one aside; none has run here.");
            }
            Ok(entries) => {
                println!("Copies set aside by a rebuild ({}):", entries.len());
                println!("  Each is what connected devices were syncing against when their");
                println!("  history was orphaned. Read one with `quarantine show <doc_id> <epoch>`.");
                println!();
                for e in entries {
                    println!("  {:<48} e{:<4} {:>8} bytes", e.doc_id, e.epoch, e.bytes);
                }
            }
            Err(e) => eprintln!("quarantine: {e}"),
        },
        Some("show") => {
            let (Some(doc_id), Some(epoch)) = (rest.first(), rest.get(1)) else {
                eprintln!("quarantine show <doc_id> <epoch>   e.g. `quarantine show chapter:abc 1`");
                return;
            };
            let Ok(epoch) = epoch.trim_start_matches('e').parse::<u64>() else {
                eprintln!("quarantine: epoch must be a number (`1`, or `e1`)");
                return;
            };
            match materialize(&crdt_dir, doc_id, epoch) {
                Ok(Some(json)) => println!("{json}"),
                Ok(None) => eprintln!("quarantine: no copy held for {doc_id} at e{epoch}"),
                Err(e) => eprintln!("quarantine: {e}"),
            }
        }
        Some(other) => {
            eprintln!("quarantine: unknown subcommand {other:?} — try `list` or `show`");
        }
    }
}

/// The `FsStore` futures resolve on first poll (rinch-storage's native backend), so a
/// single poll drives them to completion — the same trick [`crate::sync`] uses.
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone_raw(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_raw, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("fs-backed storage future unexpectedly pended"),
    }
}
