//! Reconciling a diverged, client-owned document (phase D→E).
//!
//! The backfill refuses to touch these — re-projecting would fork a disjoint history —
//! so this is the only path that resolves them, and it resolves them in whichever
//! direction a human chose. Both directions are exercised, plus the dry run, because a
//! tool that rewrites someone's prose should be provably inert until told otherwise.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_server::reconcile::{run_all, Prefer};
use serde_json::json;

/// A yrs peer, as the real client is for bodies.
struct BodyDevice {
    doc: yrs::Doc,
}

impl BodyDevice {
    fn new() -> Self {
        BodyDevice { doc: yrs::Doc::new() }
    }

    fn from_update(update: &[u8]) -> Self {
        use yrs::updates::decoder::Decode;
        use yrs::Transact;
        let doc = yrs::Doc::new();
        let update = yrs::Update::decode_v1(update).expect("decodable document");
        doc.transact_mut().apply_update(update).expect("apply");
        BodyDevice { doc }
    }

    fn state_vector(&self) -> Vec<u8> {
        use yrs::updates::encoder::Encode;
        use yrs::{ReadTxn, Transact};
        self.doc.transact().state_vector().encode_v1()
    }

    fn text(&self, key: &str) -> Option<String> {
        use yrs::{Map, Transact};
        let map = self.doc.get_or_insert_map("content");
        let txn = self.doc.transact();
        map.get(&txn, key).map(|v| v.to_string(&txn))
    }
    fn full(&self) -> Vec<u8> {
        use yrs::{ReadTxn, StateVector, Transact};
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }
}

/// A book + chapter whose canonical copy a client owns and which disagrees with git.
async fn diverged_chapter(app: &mut TestApp) -> (String, String) {
    let book_id = app.create_book("Reconcile Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": "What git believes." }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // A client claims the document with different content.
    let device = BodyDevice::new();
    let bytes = plotweb_crdt::project_body("What the CRDT believes.", plotweb_crdt::BodyKind::Chapter)
        .expect("project");
    let _ = device;
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt"),
            &bytes,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    (book_id, chapter_id)
}

#[tokio::test]
async fn a_dry_run_changes_nothing() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = diverged_chapter(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        true,
    )
    .await
    .expect("dry run");
    assert_eq!(summary.resolved.len(), 1, "it reports what it would do: {summary:?}");

    // Git untouched...
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        r.json["content"].as_str().unwrap().contains("What git believes"),
        "git is unchanged by a dry run"
    );
    // ...and the divergence is still there.
    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert_eq!(after.diverged.len(), 1, "a dry run resolves nothing: {after:?}");
}

#[tokio::test]
async fn preferring_git_replaces_the_canonical_copy_and_releases_ownership() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (_book_id, chapter_id) = diverged_chapter(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        false,
    )
    .await
    .expect("reconcile");
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");
    assert!(summary.errors.is_empty(), "{summary:?}");

    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(
        after.diverged.is_empty() && after.stale.is_empty(),
        "the two copies now agree: {after:?}"
    );

    // Ownership was released, so the backfill will maintain this document again — the
    // thing that stops it drifting once more with nothing able to fix it.
    let backfill = plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");
    assert_eq!(
        backfill.skipped_synced, 0,
        "no document should still be client-owned: {backfill:?}"
    );
    let _ = chapter_id;
}

#[tokio::test]
async fn preferring_the_crdt_writes_the_stored_document_into_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = diverged_chapter(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Crdt,
        false,
    )
    .await
    .expect("reconcile");
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");

    // Git now serves the client's text, through the ordinary read path.
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    let content = r.json["content"].as_str().unwrap().to_string();
    assert!(
        content.contains("What the CRDT believes"),
        "git took the stored document's content: {content}"
    );

    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(after.is_clean(), "and the two copies agree: {after:?}");
}

#[tokio::test]
async fn a_client_still_holding_the_pre_reconcile_document_is_refused_not_merged() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = diverged_chapter(&mut app).await;
    let uri = format!("/api/books/{book_id}/sync/chapter:{chapter_id}");

    // The device that owns the document, as it stands before anyone reconciles.
    let (_status, before) = app.get_bytes(&uri).await;
    let client = BodyDevice::from_update(&before);

    // A human decides git is right.
    run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        false,
    )
    .await
    .expect("reconcile");

    // The client's copy now descends from nothing the server holds. Clearing ownership
    // stops the *server* merging them; this is what stops the client pushing its stale
    // copy back — without it, the reconcile would be quietly undone.
    let (status, _) = app.post_bytes(&uri, &client.state_vector()).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a client holding the pre-reconcile document must be told to replace it"
    );

    // And the reconcile stands: the corpus is clean, so the stale copy did not get
    // pushed back over git's text.
    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(after.is_clean(), "and the corpus stays clean: {after:?}");
}

#[tokio::test]
async fn the_boot_hook_defaults_to_a_dry_run_for_anything_it_does_not_recognise() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = diverged_chapter(&mut app).await;

    // A typo in an environment variable must not rewrite prose.
    plotweb_server::reconcile::run_on_boot(
        "gti",
        app.book_dir().to_str().unwrap().to_string(),
        app.crdt_dir().to_str().unwrap().to_string(),
    )
    .await;

    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert!(
        r.json["content"].as_str().unwrap().contains("What git believes"),
        "an unrecognised setting resolves nothing"
    );
    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert_eq!(after.diverged.len(), 1, "still diverged: {after:?}");
}

#[tokio::test]
async fn the_boot_hook_resolves_when_given_a_direction() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (_book_id, _chapter_id) = diverged_chapter(&mut app).await;

    plotweb_server::reconcile::run_on_boot(
        "git",
        app.book_dir().to_str().unwrap().to_string(),
        app.crdt_dir().to_str().unwrap().to_string(),
    )
    .await;

    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(after.is_clean(), "resolved on boot: {after:?}");
}

// ── Structure documents (`book:`) ────────────────────────────────────────────
//
// These used to be skipped with a note to "clear ownership and re-run the backfill" —
// a remedy nothing implemented. A diverged structure document is the shape of "something
// went wrong and we cannot fix it", so both directions are exercised here the way the
// body cases are.

/// A book whose `book:` document a client owns and which disagrees with git: on the
/// device the chapter carries a different title.
async fn diverged_structure(app: &mut TestApp) -> (String, String) {
    let book_id = app.create_book("Structure Reconcile").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;

    let mut input = plotweb_server::structure::read_structure_input(&app.state().books, &book_id)
        .await
        .expect("structure in git");
    input.chapters = vec![(chapter_id.clone(), "Renamed on a device".to_string())];
    let bytes = plotweb_crdt::project_book_structure(&input).expect("project");

    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/book:{book_id}/adopt"),
            &bytes,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    (book_id, chapter_id)
}

#[tokio::test]
async fn a_structure_document_is_no_longer_skipped() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, _) = diverged_structure(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Crdt,
        true,
    )
    .await
    .expect("dry run");

    assert!(
        summary.skipped.is_empty(),
        "a structure document has a supported repair now: {summary:?}"
    );
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");
    let (doc_id, action) = &summary.resolved[0];
    assert_eq!(doc_id, &format!("book:{book_id}"));
    assert!(
        action.contains("would write") && action.contains("renamed"),
        "the dry run says what it would do: {action}"
    );

    // And it did nothing: git still has the original title...
    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    assert_eq!(r.json[0]["title"].as_str().unwrap(), "One");
    // ...and the divergence is untouched.
    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert_eq!(after.diverged.len(), 1, "a dry run resolves nothing: {after:?}");
}

#[tokio::test]
async fn preferring_git_replaces_the_canonical_structure_and_releases_ownership() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, _) = diverged_structure(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        false,
    )
    .await
    .expect("reconcile");
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");
    assert!(summary.errors.is_empty(), "{summary:?}");

    // Git keeps its title, and the canonical copy now says the same thing.
    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    assert_eq!(r.json[0]["title"].as_str().unwrap(), "One");
    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(
        after.diverged.is_empty() && after.stale.is_empty(),
        "the two copies now agree: {after:?}"
    );

    // Ownership released — which is the half the old advice named and nothing did. The
    // backfill maintains the document again.
    let backfill = plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");
    assert_eq!(
        backfill.skipped_synced, 0,
        "no document should still be client-owned: {backfill:?}"
    );
}

#[tokio::test]
async fn preferring_the_crdt_writes_the_stored_structure_into_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, _) = diverged_structure(&mut app).await;

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Crdt,
        false,
    )
    .await
    .expect("reconcile");
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");
    assert!(summary.errors.is_empty(), "{summary:?}");

    // Git took the device's title, through the ordinary read path.
    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    assert_eq!(
        r.json[0]["title"].as_str().unwrap(),
        "Renamed on a device",
        "git took the stored structure: {:?}",
        r.json
    );

    let after = plotweb_server::shadow::run_shadow_pass(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("shadow");
    assert!(after.is_clean(), "and the two copies agree: {after:?}");
}

#[tokio::test]
async fn preferring_the_crdt_can_empty_a_book_the_mirror_alone_refuses_to() {
    // The mirror will not empty a manuscript on a background pass — a canonical
    // structure with no chapters is far more likely to be half-written than an author
    // deleting their whole book. Its log says to "reconcile this deliberately", and this
    // is that: chosen by a human, at the command line, with a dry run available first.
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Emptied Book").await;
    let _ = app.create_chapter(&book_id, "One").await;

    let mut input = plotweb_server::structure::read_structure_input(&app.state().books, &book_id)
        .await
        .expect("structure in git");
    input.chapters = Vec::new();
    let bytes = plotweb_crdt::project_book_structure(&input).expect("project");
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/book:{book_id}/adopt"),
            &bytes,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Crdt,
        false,
    )
    .await
    .expect("reconcile");
    assert_eq!(summary.resolved.len(), 1, "{summary:?}");
    assert!(summary.errors.is_empty(), "{summary:?}");

    let r = app.get(&format!("/api/books/{book_id}/chapters")).await;
    assert_eq!(
        r.json.as_array().map(|a| a.len()),
        Some(0),
        "the deletion the author chose reached git: {:?}",
        r.json
    );
}

// ── Documents that cannot be read at all ────────────────────────────────────
//
// The shadow pass has always had a bucket for these ("a blob from before a CRDT
// change") and nothing ever fixed them. When the editor's collab seam moved from
// Automerge to yrs, that turned every stored body into one the server could not open —
// reads silently fell back to git, writes were dropped in favour of a sync engine that
// could not deliver, and no command in the tree would rebuild them.

/// `FsStore` percent-encodes `:` and `/` in a key to get a flat filename.
fn blob_path(app: &TestApp, key: &str) -> std::path::PathBuf {
    app.crdt_dir()
        .join(key.replace(':', "%3A").replace('/', "%2F"))
}

/// A claimed chapter whose canonical bytes this build cannot read.
async fn unreadable_chapter(app: &mut TestApp) -> (String, String) {
    let (book_id, chapter_id) = diverged_chapter(app).await;
    let doc_id = format!("chapter:{chapter_id}");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(blob_path(app, &format!("{doc_id}/manifest"))).expect("manifest"),
    )
    .expect("manifest json");
    let generation = manifest["generation"].as_str().unwrap_or_default().to_string();
    let key = if generation.is_empty() {
        format!("{doc_id}/snapshot")
    } else {
        format!("{doc_id}/{generation}/snapshot")
    };
    std::fs::write(blob_path(app, &key), b"a blob from before a CRDT change")
        .expect("overwrite snapshot");
    (book_id, chapter_id)
}

#[tokio::test]
async fn an_unreadable_document_is_rebuilt_from_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (_book_id, chapter_id) = unreadable_chapter(&mut app).await;
    let doc_id = format!("chapter:{chapter_id}");

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        // Deliberately the direction that cannot apply: there is no readable stored
        // copy to prefer, so the rebuild has to happen anyway.
        Prefer::Crdt,
        false,
    )
    .await
    .expect("reconcile");

    assert!(
        summary.rebuilt.iter().any(|(d, _)| d == &doc_id),
        "an unreadable document must be rebuilt, not reported and left: {summary:?}"
    );

    let bytes = plotweb_server::sync::canonical_snapshot(app.crdt_dir(), &doc_id)
        .expect("store read")
        .expect("a canonical copy after the rebuild");
    let content = plotweb_crdt::materialize_body(&bytes).expect("the rebuild must be readable");
    assert!(
        content.contains("What git believes."),
        "the rebuilt document carries git's content: {content}"
    );
}

#[tokio::test]
async fn a_rebuilt_document_is_provisional_again() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = unreadable_chapter(&mut app).await;
    let doc_id = format!("chapter:{chapter_id}");

    run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        false,
    )
    .await
    .expect("reconcile");

    // Ownership cleared: the next client claims the document afresh rather than
    // merging its own history into a copy that shares none.
    let heads = app.get(&format!("/api/books/{book_id}/sync/heads")).await;
    assert!(
        heads.json.get(&doc_id).is_none(),
        "a rebuilt document must be provisional again: {}",
        heads.json
    );
}

#[tokio::test]
async fn a_dry_run_leaves_an_unreadable_document_alone() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (_book_id, chapter_id) = unreadable_chapter(&mut app).await;
    let doc_id = format!("chapter:{chapter_id}");

    let summary = run_all(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
        Prefer::Git,
        true,
    )
    .await
    .expect("reconcile");

    assert!(
        summary.rebuilt.iter().any(|(d, a)| d == &doc_id && a.starts_with("would")),
        "a dry run reports the rebuild without doing it: {summary:?}"
    );
    let bytes = plotweb_server::sync::canonical_snapshot(app.crdt_dir(), &doc_id)
        .expect("store read")
        .expect("snapshot");
    assert!(
        plotweb_crdt::materialize_body(&bytes).is_err(),
        "the unreadable copy is still there after a dry run"
    );
}
