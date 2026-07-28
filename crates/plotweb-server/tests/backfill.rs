//! Integration test for the Phase C canonical Automerge backfill.
//!
//! Drives the real HTTP app to create books/chapters/notes over git storage, then
//! runs the lock-free [`plotweb_server::backfill::run_content_backfill`] over that
//! same `DATA_DIR` into a throwaway CRDT store, and asserts:
//!
//! - clean docs get a blob (`{doc_id}/snapshot`), a flagged doc does not,
//! - the summary counts match,
//! - a re-run skips everything as unchanged (idempotent),
//! - `DATA_DIR` is byte-identical before and after (git untouched — only the CRDT
//!   store is written).

mod common;

use std::collections::BTreeMap;

use common::TestApp;
use rinch_storage::{FsStore, Store};

/// A minimal single-poll block-on: the native `FsStore` futures resolve on first poll.
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
        Poll::Pending => panic!("fs store future pended"),
    }
}

/// Recursively fingerprint a directory tree: relative path → sha256 of contents.
fn tree_fingerprint(root: &std::path::Path) -> BTreeMap<String, String> {
    use sha2::{Digest, Sha256};
    let mut out = BTreeMap::new();
    fn walk(base: &std::path::Path, dir: &std::path::Path, out: &mut BTreeMap<String, String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let Ok(bytes) = std::fs::read(&p) {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                out.insert(rel, hex::encode(Sha256::digest(&bytes)));
            }
        }
    }
    walk(root, root, &mut out);
    out
}

#[tokio::test]
async fn backfill_writes_clean_blobs_and_leaves_git_untouched() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Backfill Book").await;

    // Two clean DocNode chapters.
    let clean_doc = r#"{"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"A clean paragraph."}]}
    ]}"#;
    let c1 = app.create_chapter(&book_id, "One").await;
    let c2 = app.create_chapter(&book_id, "Two").await;
    for id in [&c1, &c2] {
        let r = app
            .put(
                &format!("/api/books/{book_id}/chapters/{id}"),
                &serde_json::json!({ "content": clean_doc }),
            )
            .await;
        assert_eq!(r.status, axum::http::StatusCode::OK, "set chapter: {}", r.json);
    }

    // One FLAGGED chapter: a legacy `> blockquote` markdown (converts, but blockquote
    // is outside the collab projection → flagged → no blob).
    let c_flagged = app.create_chapter(&book_id, "Quoted").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{c_flagged}"),
            &serde_json::json!({ "content": "A line before.\n> a quoted line\nA line after." }),
        )
        .await;
    assert_eq!(r.status, axum::http::StatusCode::OK);

    // One clean legacy `<br>` note (splits at the break → clean).
    let n1 = app
        .post(
            &format!("/api/books/{book_id}/notes"),
            &serde_json::json!({ "title": "Note" }),
        )
        .await;
    assert_eq!(n1.status, axum::http::StatusCode::CREATED, "{}", n1.json);
    let n1_id = n1.id();
    let r = app
        .put(
            &format!("/api/books/{book_id}/notes/{n1_id}"),
            &serde_json::json!({ "content": "<p>First.<br>Second.</p>" }),
        )
        .await;
    assert_eq!(r.status, axum::http::StatusCode::OK, "set note: {}", r.json);

    let data_dir = app.book_dir().to_string_lossy().into_owned();

    // ── Checksum DATA_DIR before the backfill ──
    let before = tree_fingerprint(app.book_dir());

    // ── Run the backfill into a throwaway CRDT store ──
    let crdt_dir = tempfile::tempdir().expect("crdt tempdir");
    let crdt_path = crdt_dir.path().to_string_lossy().into_owned();
    let summary = plotweb_server::backfill::run_content_backfill(&data_dir, &crdt_path)
        .await
        .expect("backfill runs");

    // Expected docs: book: + 3 chapters + 1 note = 5 seen; 1 chapter flagged.
    assert_eq!(summary.books, 1, "one book");
    assert_eq!(summary.docs_seen, 5, "book + 3 chapters + 1 note");
    assert_eq!(summary.written, 4, "book + 2 clean chapters + 1 clean note");
    assert_eq!(summary.skipped_flagged, 1, "the blockquote chapter");
    assert_eq!(summary.skipped_unchanged, 0, "first run writes everything");
    assert_eq!(summary.flagged.len(), 1);
    assert!(
        summary.flagged[0].0 == format!("chapter:{c_flagged}")
            && summary.flagged[0].1.contains("blockquote"),
        "flagged doc must be the blockquote chapter: {:?}",
        summary.flagged
    );

    // ── DATA_DIR must be byte-identical (git untouched) ──
    let after = tree_fingerprint(app.book_dir());
    assert_eq!(before, after, "git DATA_DIR must be unchanged by the backfill");

    // ── Blob assertions: clean docs have a snapshot, the flagged one does not ──
    let store = FsStore::open(crdt_dir.path()).unwrap();
    let has_snapshot = |doc_id: &str| -> bool {
        block_on(store.get(&format!("{doc_id}/snapshot")))
            .unwrap()
            .is_some()
    };
    assert!(has_snapshot(&format!("book:{book_id}")), "book blob");
    assert!(has_snapshot(&format!("chapter:{c1}")), "chapter 1 blob");
    assert!(has_snapshot(&format!("chapter:{c2}")), "chapter 2 blob");
    assert!(has_snapshot(&format!("note:{n1_id}")), "note blob");
    assert!(
        !has_snapshot(&format!("chapter:{c_flagged}")),
        "flagged chapter must have NO blob"
    );

    // The written blob has real snapshot bytes (loadability as an Automerge CRDT is
    // proven in plotweb-crdt's `project_body_blob_materializes_to_expected_docnode`).
    let bytes = block_on(store.get(&format!("chapter:{c1}/snapshot")))
        .unwrap()
        .expect("chapter 1 snapshot bytes");
    assert!(!bytes.is_empty(), "snapshot blob must have bytes");

    // ── Idempotency: a second run skips everything as unchanged ──
    let summary2 = plotweb_server::backfill::run_content_backfill(&data_dir, &crdt_path)
        .await
        .expect("re-run");
    assert_eq!(summary2.written, 0, "re-run writes nothing");
    assert_eq!(
        summary2.skipped_unchanged, 4,
        "re-run skips the 4 clean docs as unchanged"
    );
    assert_eq!(summary2.skipped_flagged, 1, "flagged stays flagged");
}
