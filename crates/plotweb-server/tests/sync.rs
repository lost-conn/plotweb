//! Integration tests for the Automerge sync endpoints (sync engine slice 1).
//!
//! These drive the **real** HTTP app end to end: a test "device" is a plain
//! `AutoCommit` + `SyncState` running Automerge's sync protocol against
//! `POST /api/books/{book_id}/sync/{doc_id}`, exactly as the client will.
//!
//! What they pin down:
//! - two devices converge through the server, including across a server restart;
//! - a document the migration backfilled reaches a fresh device;
//! - the backfill never re-projects a doc a client has synced (the duplicate-history
//!   trap, `docs/sync-engine-design.md` §D8);
//! - authorization: another user's book, an unknown doc-id, and an anonymous caller
//!   are all rejected, and a malformed message is a 400 that writes nothing.

mod common;

use automerge::sync::{Message as SyncMessage, State as SyncState, SyncDoc};
use automerge::transaction::Transactable;
use automerge::{AutoCommit, ReadDoc, ROOT};
use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

/// One local device: just its document. A `SyncState` is deliberately **not** kept
/// between polls — see `sync.rs`'s module docs: a client that keeps it believes it is
/// still converged and stops asking, so it never sees another device's changes.
struct Device {
    doc: AutoCommit,
}

impl Device {
    fn new() -> Self {
        Device {
            doc: AutoCommit::new(),
        }
    }

    /// One poll cycle: fresh sync state, then round-trip until **we** have nothing
    /// further to send (the server is stateless, so it always replies).
    async fn sync(&mut self, app: &mut TestApp, uri: &str) {
        let mut state = SyncState::new();
        let mut rounds = 0;
        // Taken in its own statement: `sync()` holds a mutable borrow of the doc that
        // must be released before the reply can be integrated.
        loop {
            let outgoing = self.doc.sync().generate_sync_message(&mut state);
            let Some(msg) = outgoing else { break };
            let (status, reply) = app.post_bytes(uri, &msg.encode()).await;
            assert_eq!(status, StatusCode::OK, "sync round failed");
            rounds += 1;
            assert!(rounds < 20, "sync did not converge in {rounds} rounds");
            if reply.is_empty() {
                break;
            }
            let reply = SyncMessage::decode(&reply).expect("decodable reply");
            self.doc
                .sync()
                .receive_sync_message(&mut state, reply)
                .expect("integrate reply");
        }
    }

    fn str_at(&self, key: &str) -> Option<String> {
        self.doc
            .get(ROOT, key)
            .ok()
            .flatten()
            .and_then(|(v, _)| v.to_str().map(str::to_string))
    }
}

/// A book + chapter over the real REST routes, returning `(book_id, chapter_id)`.
async fn book_with_chapter(app: &mut TestApp) -> (String, String) {
    let book_id = app.create_book("Sync Book").await;
    let chapter_id = app.create_chapter(&book_id, "Chapter One").await;
    (book_id, chapter_id)
}

fn chapter_uri(book_id: &str, chapter_id: &str) -> String {
    format!("/api/books/{book_id}/sync/chapter:{chapter_id}")
}

#[tokio::test]
async fn two_devices_converge_through_the_server() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;
    let uri = chapter_uri(&book_id, &chapter_id);

    // Device A writes offline, then syncs.
    let mut a = Device::new();
    a.doc.put(ROOT, "note", "written on the desktop").unwrap();
    a.sync(&mut app, &uri).await;

    // Device B starts empty and pulls.
    let mut b = Device::new();
    b.sync(&mut app, &uri).await;
    assert_eq!(b.str_at("note").as_deref(), Some("written on the desktop"));

    // B edits; A pulls B's edit back down.
    b.doc.put(ROOT, "reply", "and on the web").unwrap();
    b.sync(&mut app, &uri).await;
    a.sync(&mut app, &uri).await;
    assert_eq!(a.str_at("reply").as_deref(), Some("and on the web"));
    assert_eq!(a.doc.get_heads(), b.doc.get_heads(), "devices converged");
}

#[tokio::test]
async fn a_synced_doc_survives_a_server_restart() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;
    let uri = chapter_uri(&book_id, &chapter_id);

    let mut a = Device::new();
    a.doc.put(ROOT, "note", "survives a deploy").unwrap();
    a.sync(&mut app, &uri).await;

    // Rebuild the app over the same on-disk stores — a restart/deploy.
    app.restart().await;

    let mut fresh = Device::new();
    fresh.sync(&mut app, &uri).await;
    assert_eq!(fresh.str_at("note").as_deref(), Some("survives a deploy"));
}

#[tokio::test]
async fn the_user_index_doc_syncs_without_a_book() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;

    let mut a = Device::new();
    a.doc.put(ROOT, "dashboard", "offline").unwrap();
    a.sync(&mut app, "/api/sync/user").await;

    let mut b = Device::new();
    b.sync(&mut app, "/api/sync/user").await;
    assert_eq!(b.str_at("dashboard").as_deref(), Some("offline"));
}

#[tokio::test]
async fn a_backfilled_document_reaches_a_fresh_device() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;

    // Give the chapter real content through the REST route (git is authoritative;
    // `plotweb-git` reads committed content, so this must go through the API).
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({
                "title": "Chapter One",
                "content": "{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"The lantern guttered.\"}]}]}"
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "update chapter: {}", r.json);

    // Migrate it into the canonical store, exactly as production does.
    let summary = plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");
    assert!(summary.written > 0, "backfill wrote nothing: {summary:?}");

    // A device that has never seen this book pulls the migrated document.
    let mut device = Device::new();
    device.sync(&mut app, &chapter_uri(&book_id, &chapter_id)).await;

    // The body projection's root is a `content` list of blocks (rinch-editor-collab).
    let (_value, content) = device
        .doc
        .get(ROOT, "content")
        .expect("readable")
        .expect("the migrated body must have arrived");
    assert!(
        device.doc.length(&content) > 0,
        "the migrated body must contain at least one block"
    );
}

#[tokio::test]
async fn the_backfilled_user_index_reaches_a_device_with_the_right_books() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_a = app.create_book("First Book").await;
    let book_b = app.create_book("Second Book").await;

    // A second account whose books must NOT appear in the first user's index.
    app.logout_local();
    app.register("someone-else", "password123").await;
    app.create_book("Not Mine").await;
    app.logout_local();
    app.login("author", "password123").await;

    // The ownership-aware pass, driven against the app's own open stores (rhypedb is
    // single-writer, so this is exactly how it runs in-process on the server).
    let summary = plotweb_server::backfill::run_user_backfill(
        &app.state().rhype,
        &app.state().books,
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("user backfill");
    assert_eq!(summary.users, 2, "one index per owner: {summary:?}");
    assert_eq!(summary.written, 2, "both indices written: {summary:?}");

    // A device pulls its own index and sees exactly its own books.
    let mut device = Device::new();
    device.sync(&mut app, "/api/sync/user").await;

    let (_v, books_obj) = device
        .doc
        .get(ROOT, "books")
        .expect("readable")
        .expect("the migrated user index must have arrived");
    let mut ids: Vec<String> = device.doc.keys(&books_obj).collect();
    ids.sort();
    let mut expected = vec![book_a, book_b];
    expected.sort();
    assert_eq!(ids, expected, "only this user's books belong in their index");

    // Re-running is idempotent (the source fingerprint is unchanged).
    let again = plotweb_server::backfill::run_user_backfill(
        &app.state().rhype,
        &app.state().books,
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("user backfill re-run");
    assert_eq!(again.written, 0, "re-run must write nothing: {again:?}");
    assert_eq!(again.skipped_synced, 1, "the synced index is client-owned now");
    assert_eq!(
        again.skipped_unchanged, 1,
        "the untouched index is skipped as unchanged"
    );

    // The route derives the doc id from the session, so the other account syncing the
    // same URL gets its own index — never this one's.
    app.logout_local();
    app.login("someone-else", "password123").await;
    let mut other = Device::new();
    other.sync(&mut app, "/api/sync/user").await;
    let (_v, other_books) = other
        .doc
        .get(ROOT, "books")
        .expect("readable")
        .expect("their own index");
    let other_ids: Vec<String> = other.doc.keys(&other_books).collect();
    assert_eq!(other_ids.len(), 1, "the other account has exactly its one book");
    assert!(
        !other_ids.iter().any(|id| expected.contains(id)),
        "one account's index must never carry another's books"
    );
}

#[tokio::test]
async fn the_backfill_never_reprojects_a_doc_a_client_has_synced() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;

    // A client syncs the chapter: the canonical doc is now client-owned.
    let mut device = Device::new();
    device.doc.put(ROOT, "written", "by the client").unwrap();
    device.sync(&mut app, &chapter_uri(&book_id, &chapter_id)).await;

    // The chapter also changes in git (the dual-write world we still live in), so the
    // backfill's src-sha gate alone would happily re-project it.
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "Chapter One", "content": "changed in git" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let summary = plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");

    assert!(
        summary.skipped_synced >= 1,
        "the synced chapter must be skipped, not re-projected: {summary:?}"
    );
    assert!(
        !summary
            .written_ids
            .contains(&format!("chapter:{chapter_id}")),
        "a synced doc must never be rewritten from git: {summary:?}"
    );

    // And the client's document is untouched by the backfill.
    let mut check = Device::new();
    check.sync(&mut app, &chapter_uri(&book_id, &chapter_id)).await;
    assert_eq!(check.str_at("written").as_deref(), Some("by the client"));
}

#[tokio::test]
async fn sync_rejects_another_users_book() {
    let mut app = TestApp::new().await;
    app.register("owner", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;

    // A second user, authenticated but not the owner.
    app.logout_local();
    app.register("intruder", "password123").await;

    let mut device = Device::new();
    device.doc.put(ROOT, "x", "y").unwrap();
    let mut state = SyncState::new();
    let msg = device
        .doc
        .sync()
        .generate_sync_message(&mut state)
        .expect("first message");
    let (status, _) = app
        .post_bytes(&chapter_uri(&book_id, &chapter_id), &msg.encode())
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sync_rejects_anonymous_callers_and_unknown_docs() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;

    let mut device = Device::new();
    device.doc.put(ROOT, "x", "y").unwrap();
    let mut state = SyncState::new();
    let msg = device
        .doc
        .sync()
        .generate_sync_message(&mut state)
        .expect("first message")
        .encode();

    // Doc ids that are not this book's documents — no implicit create.
    for doc_id in [
        "chapter:00000000-0000-0000-0000-000000000000",
        "note:00000000-0000-0000-0000-000000000000",
        "book:00000000-0000-0000-0000-000000000000",
        "nonsense",
    ] {
        let (status, _) = app
            .post_bytes(&format!("/api/books/{book_id}/sync/{doc_id}"), &msg)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "doc_id {doc_id} must be rejected");
    }

    // Anonymous.
    app.logout_local();
    let (status, _) = app
        .post_bytes(&chapter_uri(&book_id, &chapter_id), &msg)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app.post_bytes("/api/sync/user", &msg).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_malformed_sync_message_is_a_400_and_creates_nothing() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;
    let uri = chapter_uri(&book_id, &chapter_id);

    let (status, _) = app.post_bytes(&uri, b"definitely not a sync message").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nothing was stored, so a real device still starts from an empty canonical doc.
    let mut device = Device::new();
    device.sync(&mut app, &uri).await;
    assert!(
        device.doc.get_heads().is_empty(),
        "a rejected message must not have created a canonical document"
    );
}
