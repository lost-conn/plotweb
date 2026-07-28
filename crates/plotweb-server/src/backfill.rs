//! Canonical Automerge backfill (Phase 2 · migration phase C) — the first WRITING
//! step of the git→Automerge migration.
//!
//! Walks every book's git storage exactly like the read-only [`crate::audit`], but
//! instead of *validating* each document it *emits* the document's canonical Automerge
//! **snapshot** (via the same [`plotweb_crdt`] projection the audit certified) into a
//! new, separate blob store. This is deliberately:
//!
//! - **Additive** — the only writes go to `PLOTWEB_CRDT_DIR`. Git (`DATA_DIR`) and
//!   rhypedb (`RHYPEDB_DATA_DIR`) are read-only here.
//! - **Reversible** — everything lands under `PLOTWEB_CRDT_DIR`; deleting that
//!   directory returns the system to git-only. Nothing else changes.
//! - **Idempotent / resumable** — each blob carries a `src-sha` (the sha256 of its raw
//!   git source). A re-run skips any doc whose source is unchanged, so re-runs are
//!   cheap and an interrupted run resumes.
//! - **Lock-free** — reads git through [`BookStore`]'s read APIs (no lock), writes only
//!   the CRDT store, so it is safe to run alongside the live server.
//!
//! Only CLEAN docs get blobs: a doc the projection flags (an unsupported block, a parse
//! failure) produces no blob and stays git-only — never a partial/garbage blob.
//!
//! `user:` indices are **deferred**: they need rhypedb ownership (which user owns which
//! book), and this lock-free `DATA_DIR` walk has no ownership. A later ownership-aware
//! pass will emit them via [`plotweb_crdt::project_user_index`].

use std::future::Future;
use std::path::PathBuf;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use plotweb_crdt::{
    project_body, project_book_structure, BodyKind, BookStructureInput,
};
use plotweb_git::BookStore;
use rinch_storage::{FsStore, Store};
use sha2::{Digest, Sha256};

/// Grand-total + per-doc outcome of a backfill run. Returned so the subcommand and the
/// tests can assert exact counts.
#[derive(Debug, Default)]
pub struct BackfillSummary {
    pub books: usize,
    pub docs_seen: usize,
    pub written: usize,
    pub skipped_unchanged: usize,
    pub skipped_flagged: usize,
    /// `(doc_id, reason)` for every flagged (git-only) doc.
    pub flagged: Vec<(String, String)>,
    /// Doc-ids that received a fresh (or refreshed) blob this run.
    pub written_ids: Vec<String>,
    /// Non-fatal store/read errors encountered (a doc skipped due to an I/O error).
    pub errors: Vec<String>,
}

/// What happened to one document.
enum DocOutcome {
    Written,
    SkippedUnchanged,
    SkippedFlagged(String),
    Error(String),
}

/// Single-poll drive of a `!Send` [`FsStore`] future to completion.
///
/// The native `FsStore` does its (blocking) work on first poll and resolves
/// immediately — see rinch-storage's native backend docs and the same pattern in
/// `plotweb-web/src/local_store.rs`. Driving it synchronously here keeps the
/// **outer** async walk `Send` (its `.await` points are only the git-read futures,
/// which run on `spawn_blocking` and are `Send`): the `!Send` storage future is
/// created and consumed entirely inside this synchronous call, never held across an
/// await. A `Pending` is unreachable for `FsStore`; we panic rather than spin.
fn block_on<F: Future>(fut: F) -> F::Output {
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

/// Hex sha256 of a source fingerprint string — the idempotency/resume gate.
fn sha256_hex(src: &str) -> String {
    let mut h = Sha256::new();
    h.update(src.as_bytes());
    hex::encode(h.finalize())
}

/// A deterministic fingerprint of a `book:` structure's git source. The `book:` doc is
/// derived from several git inputs (not one content string), so we hash a canonical,
/// order-stable rendering of the structure inputs: any change re-projects, no change
/// skips. Maps are emitted in sorted-key order; order-bearing lists keep their order.
fn book_fingerprint(input: &BookStructureInput) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "title\t{}", input.title);
    let _ = writeln!(s, "description\t{}", input.description);
    let font =
        serde_json::to_string(&input.font_settings.clone().unwrap_or_default()).unwrap_or_default();
    let _ = writeln!(s, "font\t{font}");
    let _ = writeln!(s, "cover\t{:?}", input.cover_ref);
    let _ = writeln!(s, "created\t{}", input.created_at);
    for (id, title) in &input.chapters {
        let _ = writeln!(s, "ch\t{id}\t{title}");
    }
    let _ = writeln!(s, "root\t{}", input.root_order.join(","));
    let mut children: Vec<_> = input.children.iter().collect();
    children.sort_by(|a, b| a.0.cmp(b.0));
    for (parent, kids) in children {
        let _ = writeln!(s, "child\t{parent}\t{}", kids.join(","));
    }
    let mut collapsed = input.collapsed.clone();
    collapsed.sort();
    let _ = writeln!(s, "collapsed\t{}", collapsed.join(","));
    let mut notes = input.notes.clone();
    notes.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, title, color) in &notes {
        let _ = writeln!(s, "note\t{id}\t{title}\t{color:?}");
    }
    s
}

/// Emit (or skip) one document's blob, idempotently.
///
/// `src` is the raw git source fingerprint; `project` is the projection result (`Ok`
/// bytes for a clean doc, `Err(reason)` for a flagged one).
///
/// Resume gate: if `{doc_id}/src-sha` already equals sha256(`src`), the doc is
/// unchanged since it was last backfilled → skip. Otherwise write the snapshot, then
/// the manifest, then the `src-sha` **last** — the sha is the commit point, so an
/// interruption before it leaves the sha absent/stale and the next run re-emits (never
/// trusting a half-written blob).
fn backfill_doc(
    store: &FsStore,
    doc_id: &str,
    doc_type: &str,
    src: &str,
    project: Result<Vec<u8>, String>,
) -> DocOutcome {
    let src_sha = sha256_hex(src);
    let sha_key = format!("{doc_id}/src-sha");

    match block_on(store.get(&sha_key)) {
        Ok(Some(existing)) if existing == src_sha.as_bytes() => {
            return DocOutcome::SkippedUnchanged;
        }
        Ok(_) => {}
        Err(e) => return DocOutcome::Error(format!("{doc_id}: read src-sha: {e}")),
    }

    let bytes = match project {
        Ok(b) => b,
        Err(reason) => return DocOutcome::SkippedFlagged(reason),
    };

    let snap_key = format!("{doc_id}/snapshot");
    let manifest_key = format!("{doc_id}/manifest");
    let manifest = serde_json::json!({
        "doc_id": doc_id,
        "type": doc_type,
        "src_sha": src_sha,
        "projection": "automerge-snapshot-v1",
    })
    .to_string();

    if let Err(e) = block_on(store.put(&snap_key, &bytes)) {
        return DocOutcome::Error(format!("{doc_id}: write snapshot: {e}"));
    }
    if let Err(e) = block_on(store.put(&manifest_key, manifest.as_bytes())) {
        return DocOutcome::Error(format!("{doc_id}: write manifest: {e}"));
    }
    // Commit point last.
    if let Err(e) = block_on(store.put(&sha_key, src_sha.as_bytes())) {
        return DocOutcome::Error(format!("{doc_id}: write src-sha: {e}"));
    }
    DocOutcome::Written
}

/// Fold one document's outcome into the running summary.
fn record(summary: &mut BackfillSummary, doc_id: &str, outcome: DocOutcome) {
    summary.docs_seen += 1;
    match outcome {
        DocOutcome::Written => {
            summary.written += 1;
            summary.written_ids.push(doc_id.to_string());
        }
        DocOutcome::SkippedUnchanged => summary.skipped_unchanged += 1,
        DocOutcome::SkippedFlagged(reason) => {
            summary.skipped_flagged += 1;
            summary.flagged.push((doc_id.to_string(), reason));
        }
        DocOutcome::Error(e) => summary.errors.push(e),
    }
}

/// Lock-free, content-only migration backfill. Enumerates books straight from the
/// `DATA_DIR` filesystem (each subdirectory holding a `manuscript/book.json` is one
/// book — exactly like [`crate::audit::run_content_audit`]) and, for each book, emits
/// the canonical Automerge snapshot for its structure, every chapter, and every note
/// into `crdt_dir`. Reads git ONLY; writes ONLY the CRDT store. `user:` indices are
/// deferred (rhypedb-owned).
pub async fn run_content_backfill(data_dir: &str, crdt_dir: &str) -> Result<BackfillSummary, String> {
    let store = FsStore::open(PathBuf::from(crdt_dir))
        .map_err(|e| format!("failed to open CRDT store at {crdt_dir}: {e}"))?;
    let books = BookStore::new(PathBuf::from(data_dir));

    let subdirs: Vec<PathBuf> = match std::fs::read_dir(data_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    let mut book_ids: Vec<String> = subdirs
        .iter()
        .filter(|p| p.join("manuscript").join("book.json").is_file())
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    book_ids.sort();
    println!(
        "[backfill] {} subdirectories under DATA_DIR, {} identified as books",
        subdirs.len(),
        book_ids.len()
    );

    let mut summary = BackfillSummary::default();

    for book_id in &book_ids {
        summary.books += 1;
        let before = (
            summary.written,
            summary.skipped_unchanged,
            summary.skipped_flagged,
        );

        // ── Read everything for this book (async → owned, Send data). ──
        let book_data = books.get_book(book_id).await;
        let chapters = books.list_chapters(book_id).await.unwrap_or_default();
        let (notes_list, notes_tree) = books.list_notes(book_id).await.unwrap_or_default();

        // book: structure — needs a readable book.json.
        let book_doc_id = format!("book:{book_id}");
        match &book_data {
            Ok(d) => {
                let input = BookStructureInput {
                    title: d.title.clone(),
                    description: d.description.clone(),
                    font_settings: d.font_settings.clone(),
                    cover_ref: d.cover_image.clone(),
                    created_at: d.created_at.clone(),
                    chapters: chapters
                        .iter()
                        .map(|c| (c.id.clone(), c.title.clone()))
                        .collect(),
                    root_order: notes_tree.root_order.clone(),
                    children: notes_tree.children.clone(),
                    collapsed: notes_tree.collapsed.clone(),
                    notes: notes_list
                        .iter()
                        .map(|n| (n.id.clone(), n.title.clone(), n.color.clone()))
                        .collect(),
                };
                let fp = book_fingerprint(&input);
                let outcome =
                    backfill_doc(&store, &book_doc_id, "book", &fp, project_book_structure(&input));
                record(&mut summary, &book_doc_id, outcome);
            }
            Err(e) => {
                // Can't read the book → can't project it. Skip as flagged (git-only),
                // never a partial blob.
                record(
                    &mut summary,
                    &book_doc_id,
                    DocOutcome::SkippedFlagged(format!("git read failed: {e}")),
                );
            }
        }

        // chapters
        for c in &chapters {
            let doc_id = format!("chapter:{}", c.id);
            let outcome = backfill_doc(
                &store,
                &doc_id,
                "chapter",
                &c.content,
                project_body(&c.content, BodyKind::Chapter),
            );
            record(&mut summary, &doc_id, outcome);
        }

        // notes
        for n in &notes_list {
            let doc_id = format!("note:{}", n.id);
            let outcome = backfill_doc(
                &store,
                &doc_id,
                "note",
                &n.content,
                project_body(&n.content, BodyKind::Note),
            );
            record(&mut summary, &doc_id, outcome);
        }

        let (w, u, f) = (
            summary.written - before.0,
            summary.skipped_unchanged - before.1,
            summary.skipped_flagged - before.2,
        );
        let title = book_data.as_ref().map(|d| d.title.as_str()).unwrap_or("<unreadable>");
        println!(
            "[backfill] book {book_id} \"{title}\": {} chapter(s), {} note(s) — \
             {w} written, {u} unchanged, {f} flagged",
            chapters.len(),
            notes_list.len()
        );
    }

    Ok(summary)
}

/// Human summary of a backfill run (grand totals + flagged list).
pub fn print_summary(data_dir: &str, crdt_dir: &str, summary: &BackfillSummary) {
    println!();
    println!("────────────────────────────────────────");
    println!("PlotWeb migration backfill (Phase C)");
    println!("  Additive + reversible: the ONLY writes go to PLOTWEB_CRDT_DIR.");
    println!("  Git storage and rhypedb are read-only. Lock-free.");
    println!("  DATA_DIR        : {data_dir}");
    println!("  PLOTWEB_CRDT_DIR: {crdt_dir}");
    println!();
    println!("  books scanned      : {}", summary.books);
    println!("  docs seen          : {}", summary.docs_seen);
    println!("  blobs written      : {}", summary.written);
    println!("  skipped-unchanged  : {}", summary.skipped_unchanged);
    println!("  skipped-flagged    : {}", summary.skipped_flagged);
    println!("  user: indices      : deferred — rhypedb-owned (ownership not in this lock-free walk)");
    if !summary.errors.is_empty() {
        println!("  store/read errors  : {}", summary.errors.len());
        for e in &summary.errors {
            println!("    ! {e}");
        }
    }
    println!();
    if summary.flagged.is_empty() {
        println!("Flagged docs (git-only, no blob): (none)");
    } else {
        println!("Flagged docs (git-only, no blob):");
        for (doc_id, reason) in &summary.flagged {
            println!("  {doc_id} — {reason}");
        }
    }
}

/// Entry point for `plotweb-server backfill-migration`.
///
/// Opens `PLOTWEB_CRDT_DIR` (default `data/crdt`), runs the lock-free content
/// backfill against `DATA_DIR`, prints the summary, and returns. Never starts the
/// server; only the CRDT store is written.
pub async fn run() {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data/books".into());
    let crdt_dir = std::env::var("PLOTWEB_CRDT_DIR").unwrap_or_else(|_| "data/crdt".into());

    println!(
        "[backfill] starting canonical Automerge backfill — additive, reversible, \
         lock-free, content only (user: indices deferred)"
    );
    match run_content_backfill(&data_dir, &crdt_dir).await {
        Ok(summary) => print_summary(&data_dir, &crdt_dir, &summary),
        Err(e) => eprintln!("backfill-migration: {e}"),
    }
    println!("[backfill] backfill complete");
}

/// Boot-time hook (env `PLOTWEB_BACKFILL_ON_BOOT`): run the lock-free content
/// backfill against the live `DATA_DIR`, writing blobs into `PLOTWEB_CRDT_DIR`,
/// concurrently with serving. Lock-free and additive (writes only the CRDT store),
/// so it is safe next to the live server.
pub async fn run_boot_backfill(data_dir: String, crdt_dir: String) {
    println!(
        "[boot-backfill] starting canonical Automerge backfill — additive, reversible, \
         lock-free, writes only PLOTWEB_CRDT_DIR"
    );
    match run_content_backfill(&data_dir, &crdt_dir).await {
        Ok(summary) => print_summary(&data_dir, &crdt_dir, &summary),
        Err(e) => eprintln!("[boot-backfill] {e}"),
    }
    println!("[boot-backfill] backfill complete");
}
