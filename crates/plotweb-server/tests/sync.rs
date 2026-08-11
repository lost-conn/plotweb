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


/// A device holding a **body** document. Bodies are yrs since rinch #190, so they
/// reconcile in two fixed steps rather than Automerge's message loop: send a state
/// vector, get back the update you lack plus the peer's state vector, then send what
/// the peer lacks.
struct BodyDevice {
    doc: yrs::Doc,
}

impl BodyDevice {
    fn new() -> Self {
        BodyDevice { doc: yrs::Doc::new() }
    }

    /// Write a root-level value, standing in for prose the editor would project.
    fn put(&self, key: &str, value: &str) {
        use yrs::{Map, Transact};
        let map = self.doc.get_or_insert_map("content");
        let mut txn = self.doc.transact_mut();
        map.insert(&mut txn, key, value);
    }

    fn get(&self, key: &str) -> Option<String> {
        use yrs::{Map, Transact};
        let map = self.doc.get_or_insert_map("content");
        let txn = self.doc.transact();
        map.get(&txn, key).map(|v| v.to_string(&txn))
    }

    /// The whole document as one update — what `adopt` takes.
    fn full(&self) -> Vec<u8> {
        use yrs::{ReadTxn, StateVector, Transact};
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    fn state_vector(&self) -> Vec<u8> {
        use yrs::updates::encoder::Encode;
        use yrs::{ReadTxn, Transact};
        self.doc.transact().state_vector().encode_v1()
    }

    /// One full exchange against the server.
    async fn sync(&self, app: &mut TestApp, uri: &str) {
        use yrs::updates::decoder::Decode;
        use yrs::{ReadTxn, StateVector, Transact};

        let (status, framed) = app.post_bytes(uri, &self.state_vector()).await;
        assert_eq!(status, StatusCode::OK, "exchange failed");

        // `[u32 LE length][diff][server state vector]`
        assert!(framed.len() >= 4, "reply is framed");
        let len = u32::from_le_bytes(framed[0..4].try_into().unwrap()) as usize;
        let diff = &framed[4..4 + len];
        let server_sv = &framed[4 + len..];

        if !diff.is_empty() {
            let update = yrs::Update::decode_v1(diff).expect("decodable diff");
            self.doc.transact_mut().apply_update(update).expect("apply");
        }

        let ours = {
            let sv = StateVector::decode_v1(server_sv).expect("decodable state vector");
            self.doc.transact().encode_diff_v1(&sv)
        };
        if !ours.is_empty() {
            let (status, _) = app.post_bytes(&format!("{uri}/update"), &ours).await;
            assert_eq!(status, StatusCode::OK, "update rejected");
        }
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
    let a = BodyDevice::new();
    a.put("note", "written on the desktop");
    a.sync(&mut app, &uri).await;

    // Device B starts empty and pulls.
    let b = BodyDevice::new();
    b.sync(&mut app, &uri).await;
    assert_eq!(b.get("note").as_deref(), Some("written on the desktop"));

    // B edits; A pulls B's edit back down.
    b.put("reply", "and on the web");
    b.sync(&mut app, &uri).await;
    a.sync(&mut app, &uri).await;
    assert_eq!(a.get("reply").as_deref(), Some("and on the web"));
    assert_eq!(a.get("note").as_deref(), Some("written on the desktop"));
}

#[tokio::test]
async fn a_synced_doc_survives_a_server_restart() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;
    let uri = chapter_uri(&book_id, &chapter_id);

    let a = BodyDevice::new();
    a.put("note", "survives a deploy");
    a.sync(&mut app, &uri).await;

    // Rebuild the app over the same on-disk stores — a restart/deploy.
    app.restart().await;

    let fresh = BodyDevice::new();
    fresh.sync(&mut app, &uri).await;
    assert_eq!(fresh.get("note").as_deref(), Some("survives a deploy"));
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
    let device = BodyDevice::new();
    device.sync(&mut app, &chapter_uri(&book_id, &chapter_id)).await;

    // The body projection's root is a `content` array of blocks (rinch-editor-collab,
    // now yrs), so a migrated chapter arrives as a non-empty array.
    let blocks = {
        use yrs::{Array, Transact};
        let content = device.doc.get_or_insert_array("content");
        let txn = device.doc.transact();
        content.len(&txn)
    };
    assert!(blocks > 0, "the migrated body must contain at least one block");
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
    let device = BodyDevice::new();
    device.put("written", "by the client");
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
    let check = BodyDevice::new();
    check.sync(&mut app, &chapter_uri(&book_id, &chapter_id)).await;
    assert_eq!(check.get("written").as_deref(), Some("by the client"));
}

#[tokio::test]
async fn the_first_client_takes_ownership_of_a_backfilled_body() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;

    // Chapter content in git, then migrated — the canonical copy is now the
    // backfill's, and it freezes here while git keeps moving.
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "Chapter One", "content": "backfill-era text" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    plotweb_server::backfill::run_content_backfill(
        app.book_dir().to_str().unwrap(),
        app.crdt_dir().to_str().unwrap(),
    )
    .await
    .expect("backfill");

    // A device holding the *current* (git-newer) body claims the document.
    let device = BodyDevice::new();
    device.put("body", "current text, newer than backfill");
    let uri = format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt");
    let (status, body) = app.post_bytes(&uri, &device.full()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["adopted"],
        serde_json::json!(true),
        "a pristine backfill blob is provisional — the first client replaces it"
    );

    // A second device syncing normally now gets the adopting device's document,
    // not the backfilled one — and no duplicate of it.
    let other = BodyDevice::new();
    other.sync(&mut app, &chapter_uri(&book_id, &chapter_id)).await;
    assert_eq!(
        other.get("body").as_deref(),
        Some("current text, newer than backfill")
    );

    // And ownership is once-only: a later claim is refused so it cannot discard the
    // peer's changes.
    let latecomer = BodyDevice::new();
    latecomer.put("body", "would clobber");
    let (status, body) = app.post_bytes(&uri, &latecomer.full()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["adopted"],
        serde_json::json!(false),
        "an owned document must be synced, never adopted over"
    );

    let check = BodyDevice::new();
    check.sync(&mut app, &chapter_uri(&book_id, &chapter_id)).await;
    assert_eq!(
        check.get("body").as_deref(),
        Some("current text, newer than backfill"),
        "the refused claim left the canonical document untouched"
    );
}

#[tokio::test]
async fn the_heads_listing_reports_only_documents_the_server_holds() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Heads Book").await;
    let synced_id = app.create_chapter(&book_id, "Synced").await;
    let untouched_id = app.create_chapter(&book_id, "Untouched").await;

    // Only one chapter is ever synced.
    let device = BodyDevice::new();
    device.put("body", "text");
    device.sync(&mut app, &chapter_uri(&book_id, &synced_id)).await;

    let r = app.get(&format!("/api/books/{book_id}/sync/heads")).await;
    assert_eq!(r.status, StatusCode::OK);
    let heads = r.json.as_object().expect("a doc-id → heads map");

    let synced_key = format!("chapter:{synced_id}");
    assert!(
        heads.contains_key(&synced_key),
        "a synced document is listed: {heads:?}"
    );
    assert_eq!(
        heads[&synced_key].as_array().map(|a| a.len()),
        Some(1),
        "with its current fingerprint (yrs has no head hashes — see body_fingerprint)"
    );
    assert!(
        !heads.contains_key(&format!("chapter:{untouched_id}")),
        "a document the server has no copy of is absent, so a client knows not to \
         bother pulling it: {heads:?}"
    );

    // Heads move when the document moves — this is what lets a sweep skip the quiet
    // ones and pick up only what changed.
    device.put("body", "text, revised");
    device.sync(&mut app, &chapter_uri(&book_id, &synced_id)).await;
    let after = app.get(&format!("/api/books/{book_id}/sync/heads")).await;
    assert_ne!(
        after.json[&synced_key], heads[&synced_key],
        "an edited document reports a new frontier"
    );

    // Another user's book is not enumerable.
    app.logout_local();
    app.register("intruder", "password123").await;
    let r = app.get(&format!("/api/books/{book_id}/sync/heads")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn adopt_rejects_junk_and_other_peoples_books() {
    let mut app = TestApp::new().await;
    app.register("owner", "password123").await;
    let (book_id, chapter_id) = book_with_chapter(&mut app).await;
    let uri = format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt");

    let (status, _) = app.post_bytes(&uri, b"not an automerge document").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:nope/adopt"),
            &AutoCommit::new().save(),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown doc id");

    app.logout_local();
    app.register("intruder", "password123").await;
    let (status, _) = app.post_bytes(&uri, &AutoCommit::new().save()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "another user's book");
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

    let (status, _) = app.post_bytes(&uri, b"definitely not a state vector").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nothing was stored, so a real device still starts from an empty canonical doc.
    let device = BodyDevice::new();
    device.sync(&mut app, &uri).await;
    assert!(
        device.get("anything").is_none(),
        "a rejected message must not have created a canonical document"
    );
}
