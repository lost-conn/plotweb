//! Carrying sync-originated writes back into git for a cut-over book (phase E).
//!
//! A REST write keeps git and the canonical document in step by itself. A sync write
//! does not — it moves the canonical copy and git never hears about it. These check
//! that the mirror closes that gap, that it stays out of the way for a book that has
//! not been cut over (where git is still the source of truth), and that it waits rather
//! than committing on every keystroke's worth of sync traffic.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::TestApp;
use plotweb_crdt::BodyKind;
use plotweb_server::mirror;
use serde_json::json;

const NOW: Duration = Duration::ZERO;
const NEVER: Duration = Duration::from_secs(86_400);

fn doc_json(text: &str) -> String {
    format!(
        r#"{{"type":"doc","content":[{{"type":"paragraph","content":[{{"type":"text","text":"{text}"}}]}}]}}"#
    )
}

/// The update a device would push after editing a document it already shares with the
/// server: a change recorded on the *same* history, encoded against what the server has.
fn edit_as_update(held: &[u8], new_content: &str) -> Vec<u8> {
    use yrs::updates::decoder::Decode;
    use yrs::{ReadTxn, Transact};

    let edited = plotweb_crdt::apply_content(held, new_content, BodyKind::Chapter).expect("edit");

    let server = yrs::Doc::new();
    server
        .transact_mut()
        .apply_update(yrs::Update::decode_v1(held).expect("decodable"))
        .expect("apply");
    let server_sv = server.transact().state_vector();

    let peer = yrs::Doc::new();
    peer.transact_mut()
        .apply_update(yrs::Update::decode_v1(&edited).expect("decodable"))
        .expect("apply");
    peer.transact().encode_diff_v1(&server_sv)
}

/// A book and chapter whose git copy and canonical copy agree, with the canonical one
/// owned by a client — the state a device is in once it has synced.
async fn synced_chapter(app: &mut TestApp, text: &str) -> (String, String, Vec<u8>) {
    let book_id = app.create_book("Mirror Book").await;
    let chapter_id = app.create_chapter(&book_id, "One").await;
    let r = app
        .put(
            &format!("/api/books/{book_id}/chapters/{chapter_id}"),
            &json!({ "title": "One", "content": doc_json(text) }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let held = plotweb_crdt::project_body(&doc_json(text), BodyKind::Chapter).expect("project");
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/adopt"),
            &held,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    (book_id, chapter_id, held)
}

async fn git_content(app: &TestApp, book_id: &str, chapter_id: &str) -> String {
    app.state()
        .books
        .get_chapter(book_id, chapter_id)
        .await
        .expect("chapter in git")
        .content
}

#[tokio::test]
async fn a_sync_write_to_a_cut_over_book_is_carried_into_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id, held) = synced_chapter(&mut app, "as both copies had it").await;
    app.cut_over(&book_id).await;

    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/update"),
            &edit_as_update(&held, &doc_json("as the device changed it")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Before the mirror runs, git is behind — but the author is served the canonical
    // copy regardless, because that is what cutover means. Git catching up is this
    // pass's job, not something the reader should ever wait on.
    assert!(
        git_content(&app, &book_id, &chapter_id)
            .await
            .contains("as both copies had it"),
        "the sync write should not have reached git on its own"
    );
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.json["content"].as_str().unwrap().contains("as the device changed it"),
        "the reader sees the device's edit immediately, not once the mirror commits"
    );

    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 1);

    let git = git_content(&app, &book_id, &chapter_id).await;
    assert!(
        git.contains("as the device changed it"),
        "git must end up holding what the device synced: {git}"
    );
    let r = app
        .get(&format!("/api/books/{book_id}/chapters/{chapter_id}"))
        .await;
    assert_eq!(r.status, StatusCode::OK, "and still reads normally afterwards");
}

#[tokio::test]
async fn a_book_that_is_not_cut_over_is_left_to_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id, held) = synced_chapter(&mut app, "what git says").await;

    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/update"),
            &edit_as_update(&held, &doc_json("what the device says")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        app.state().mirror.is_empty(),
        "git is still the source of truth here — the CRDT does not get to write it"
    );
    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 0);
    assert!(
        git_content(&app, &book_id, &chapter_id)
            .await
            .contains("what git says"),
        "nothing may write git behind the REST path for a book that isn't cut over"
    );
}

#[tokio::test]
async fn a_document_still_being_edited_is_not_committed_yet() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id, held) = synced_chapter(&mut app, "first").await;
    app.cut_over(&book_id).await;

    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/update"),
            &edit_as_update(&held, &doc_json("second")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // A sync round arrives every second or two while someone types. Committing each one
    // would bury the book's history in noise, so the document waits until it is quiet.
    assert_eq!(mirror::flush(app.state(), NEVER, NEVER).await, 0);
    assert_eq!(app.state().mirror.len(), 1, "and it is still owed");
    assert!(git_content(&app, &book_id, &chapter_id).await.contains("first"));

    assert_eq!(mirror::flush(app.state(), NOW, NEVER).await, 1);
    assert!(git_content(&app, &book_id, &chapter_id).await.contains("second"));
}

#[tokio::test]
async fn a_long_editing_session_is_checkpointed_rather_than_starved() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id, held) = synced_chapter(&mut app, "first").await;
    app.cut_over(&book_id).await;

    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/update"),
            &edit_as_update(&held, &doc_json("second")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Never idle, but past the ceiling: without this an author who types for an hour
    // gets no git copy for an hour, and "git is a live mirror" is a promise the mirror
    // does not keep.
    assert_eq!(mirror::flush(app.state(), NEVER, NOW).await, 1);
    assert!(git_content(&app, &book_id, &chapter_id).await.contains("second"));
}

#[tokio::test]
async fn a_sync_round_that_changes_nothing_leaves_no_commit() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, chapter_id, held) = synced_chapter(&mut app, "unchanged").await;
    app.cut_over(&book_id).await;

    // Re-sending what the server already has: the queue may be marked, but there is
    // nothing to write, and an author's history should not gain a commit for it.
    let before = app.state().books.list_commits(&book_id, 50, 0).await.expect("history").len();
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/chapter:{chapter_id}/update"),
            &edit_as_update(&held, &doc_json("unchanged")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    mirror::flush(app.state(), NOW, NOW).await;

    let after = app.state().books.list_commits(&book_id, 50, 0).await.expect("history").len();
    assert_eq!(before, after, "a no-op sync must not appear in the book's history");
}

#[tokio::test]
async fn a_synced_note_is_mirrored_too() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let book_id = app.create_book("Mirror Book").await;
    let r = app
        .post(
            &format!("/api/books/{book_id}/notes"),
            &json!({ "title": "Idea", "parent_id": null, "color": null }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let note_id = r.id();
    let r = app
        .put(
            &format!("/api/books/{book_id}/notes/{note_id}"),
            &json!({ "content": doc_json("as both copies had it") }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let held = plotweb_crdt::project_body(&doc_json("as both copies had it"), BodyKind::Note)
        .expect("project");
    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/note:{note_id}/adopt"),
            &held,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    app.cut_over(&book_id).await;

    // Notes live in their own repository, reached through a different write call than
    // chapters — the branch is worth its own test rather than assumed from the chapter.
    use yrs::updates::decoder::Decode;
    use yrs::{ReadTxn, Transact};
    let edited =
        plotweb_crdt::apply_content(&held, &doc_json("as the device changed it"), BodyKind::Note)
            .expect("edit");
    let server = yrs::Doc::new();
    server
        .transact_mut()
        .apply_update(yrs::Update::decode_v1(&held).expect("decodable"))
        .expect("apply");
    let server_sv = server.transact().state_vector();
    let peer = yrs::Doc::new();
    peer.transact_mut()
        .apply_update(yrs::Update::decode_v1(&edited).expect("decodable"))
        .expect("apply");
    let update = peer.transact().encode_diff_v1(&server_sv);

    let (status, _) = app
        .post_bytes(
            &format!("/api/books/{book_id}/sync/note:{note_id}/update"),
            &update,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 1);

    let git = app
        .state()
        .books
        .get_note(&book_id, &note_id)
        .await
        .expect("note in git")
        .content;
    assert!(
        git.contains("as the device changed it"),
        "a note's git copy must follow its canonical one too: {git}"
    );
}

// ── Structure ────────────────────────────────────────────────────────────────
//
// A device can change a book's shape as well as its prose — add a chapter, rename one,
// reorder, reparent a note. Those arrive as changes to the `book:` document and, like
// bodies, never touch git on their own.

use automerge::{sync::State as SyncState, sync::SyncDoc, AutoCommit};
use plotweb_crdt::BookStructureInput;

struct Device {
    doc: AutoCommit,
}

impl Device {
    /// One poll cycle against a structure document.
    async fn sync(&mut self, app: &mut TestApp, book_id: &str) {
        use automerge::sync::Message as SyncMessage;
        let uri = format!("/api/books/{book_id}/sync/book:{book_id}");
        let mut state = SyncState::new();
        let mut rounds = 0;
        loop {
            let outgoing = self.doc.sync().generate_sync_message(&mut state);
            let Some(msg) = outgoing else { break };
            let (status, reply) = app.post_bytes(&uri, &msg.encode()).await;
            assert_eq!(status, StatusCode::OK);
            rounds += 1;
            assert!(rounds < 20, "sync did not converge");
            if reply.is_empty() {
                break;
            }
            let reply = SyncMessage::decode(&reply).expect("decodable");
            self.doc
                .sync()
                .receive_sync_message(&mut state, reply)
                .expect("integrate");
        }
    }
}

/// Pull the canonical structure onto a device, change it there, and push it back —
/// which is all a real device does when someone adds a chapter offline.
async fn device_changes(
    app: &mut TestApp,
    book_id: &str,
    edit: impl FnOnce(&mut BookStructureInput),
) {
    let mut device = Device { doc: AutoCommit::new() };
    device.sync(app, book_id).await;

    let mut input = plotweb_server::structure::read_structure_input(&app.state().books, book_id)
        .await
        .expect("structure in git");
    let removable = removed_by(&mut input, edit);

    let changed = plotweb_crdt::apply_book_structure(&device.doc.save(), &input, &removable)
        .expect("device edit");
    device.doc = AutoCommit::load(&changed).expect("reload");
    device.sync(app, book_id).await;
}

async fn git_structure(app: &TestApp, book_id: &str) -> plotweb_crdt::BookStructure {
    plotweb_server::structure::read_structure_input(&app.state().books, book_id)
        .await
        .expect("structure in git")
        .structure()
}

/// A cut-over book with two chapters and a note, whose canonical structure exists.
async fn structured_book(app: &mut TestApp) -> (String, String, String, String) {
    let book_id = app.create_book("Mirror Structure").await;
    app.cut_over(&book_id).await;
    let c1 = app.create_chapter(&book_id, "One").await;
    let c2 = app.create_chapter(&book_id, "Two").await;
    let r = app
        .post(
            &format!("/api/books/{book_id}/notes"),
            &json!({ "title": "Characters", "parent_id": null, "color": null }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    (book_id, c1, c2, r.id())
}

#[tokio::test]
async fn a_chapter_added_on_a_device_reaches_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, c1, c2, _) = structured_book(&mut app).await;

    device_changes(&mut app, &book_id, |input| {
        input.chapters.push(("c-new".into(), "Three".into()));
    })
    .await;

    assert!(
        !git_structure(&app, &book_id)
            .await
            .chapters
            .iter()
            .any(|(id, _)| id == "c-new"),
        "the device's change should not have reached git on its own"
    );

    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 1);
    assert_eq!(
        git_structure(&app, &book_id).await.chapters,
        vec![
            (c1, "One".into()),
            (c2, "Two".into()),
            ("c-new".to_string(), "Three".to_string())
        ],
        "the chapter must exist in git, in the order the device put it"
    );
}

#[tokio::test]
async fn a_rename_a_reorder_and_a_deletion_all_reach_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, c1, c2, _) = structured_book(&mut app).await;
    let c3 = app.create_chapter(&book_id, "Three").await;

    let (r1, r2) = (c1.clone(), c2.clone());
    device_changes(&mut app, &book_id, move |input| {
        input.chapters.retain(|(id, _)| id != &r1);
        input.chapters.reverse();
        for (id, title) in input.chapters.iter_mut() {
            if id == &r2 {
                *title = "Two, revised".into();
            }
        }
    })
    .await;

    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 1);
    assert_eq!(
        git_structure(&app, &book_id).await.chapters,
        vec![(c3, "Three".into()), (c2, "Two, revised".into())],
        "a deletion is carried through too — git keeps every past version of the file, \
         so mirroring one is no more destructive than deleting it in the browser"
    );
}

#[tokio::test]
async fn a_notes_tree_rearranged_on_a_device_reaches_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, _, _, n1) = structured_book(&mut app).await;

    let parent = n1.clone();
    device_changes(&mut app, &book_id, move |input| {
        input.notes.push(("n-new".into(), "Alice".into(), Some("teal".into())));
        input.children.insert(parent.clone(), vec!["n-new".into()]);
        input.collapsed.push(parent);
    })
    .await;

    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 1);
    let git = git_structure(&app, &book_id).await;
    assert_eq!(git.note_titles.get("n-new").map(String::as_str), Some("Alice"));
    assert_eq!(git.note_colors.get("n-new").map(String::as_str), Some("teal"));
    assert_eq!(git.children.get(&n1), Some(&vec!["n-new".to_string()]));
    assert!(git.collapsed.contains(&n1));
}

#[tokio::test]
async fn book_metadata_changed_on_a_device_reaches_git() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, _, _, _) = structured_book(&mut app).await;

    device_changes(&mut app, &book_id, |input| {
        input.title = "Mirror Structure, Revised".into();
        input.description = "written on the other device".into();
    })
    .await;

    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 1);
    let git = git_structure(&app, &book_id).await;
    assert_eq!(git.title, "Mirror Structure, Revised");
    assert_eq!(git.description, "written on the other device");
}

#[tokio::test]
async fn a_canonical_structure_that_lost_everything_is_refused() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, c1, c2, _) = structured_book(&mut app).await;

    device_changes(&mut app, &book_id, |input| {
        input.chapters.clear();
    })
    .await;

    // Far more likely a half-written document than an author who deleted every chapter
    // — and they can do that through the UI, which needs no help from this pass.
    assert_eq!(mirror::flush(app.state(), NOW, NOW).await, 0);
    assert_eq!(
        git_structure(&app, &book_id).await.chapters,
        vec![(c1, "One".into()), (c2, "Two".into())],
        "the manuscript must still be there"
    );
}

#[tokio::test]
async fn a_structure_sync_that_changes_nothing_leaves_no_commit() {
    let mut app = TestApp::new().await;
    app.register("author", "password123").await;
    let (book_id, _, _, _) = structured_book(&mut app).await;

    let before = app.state().books.list_commits(&book_id, 50, 0).await.expect("history").len();
    let mut device = Device { doc: AutoCommit::new() };
    device.sync(&mut app, &book_id).await;
    mirror::flush(app.state(), NOW, NOW).await;

    let after = app.state().books.list_commits(&book_id, 50, 0).await.expect("history").len();
    assert_eq!(before, after, "a pull must not write anything back");
}

/// Apply `edit`, returning the ids it removed — the same thing a real caller has to
/// state explicitly, since absence from git alone is not evidence of deletion (see
/// `plotweb_crdt::apply_book_structure`).
fn removed_by(
    input: &mut plotweb_crdt::BookStructureInput,
    edit: impl FnOnce(&mut plotweb_crdt::BookStructureInput),
) -> Vec<String> {
    let ids = |i: &plotweb_crdt::BookStructureInput| -> Vec<String> {
        i.chapters
            .iter()
            .map(|(id, _)| id.clone())
            .chain(i.notes.iter().map(|(id, _, _)| id.clone()))
            .collect()
    };
    let before = ids(input);
    edit(input);
    let after = ids(input);
    before.into_iter().filter(|id| !after.contains(id)).collect()
}
