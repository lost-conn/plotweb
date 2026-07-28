//! User-index round-trip (`user:`) — schema §1.
//!
//! Builds the `user:` Automerge doc (a `books` map of cached dashboard entries —
//! `title` / `cover_ref?` / `updated_at`), persists + reloads it, projects it back
//! (applying the dashboard's newest-first sort), and asserts it equals the input.
//! Mirrors `plotweb-web/src/local_user.rs`.

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ROOT, ReadDoc};

use crate::RoundTrip;

/// Raw inputs for a `user:` index doc: one cached entry per book the user owns.
#[derive(Clone, Debug)]
pub struct UserIndexInput {
    /// `(book_id, title, cover_ref?, updated_at)`. `updated_at` is the
    /// lexicographically-sortable `"%Y-%m-%d %H:%M:%S"` string.
    pub books: Vec<(String, String, Option<String>, String)>,
}

/// A cached entry as read back, in the dashboard's render order.
#[derive(Debug, PartialEq, Eq)]
struct EntryNorm {
    id: String,
    title: String,
    cover_ref: Option<String>,
    updated_at: String,
}

/// Newest-first, id tie-break — exactly `local_user::project_books`.
fn sort_entries(entries: &mut [EntryNorm]) {
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
}

/// Round-trip a `user:` index: build the doc, persist + reload, project back in
/// dashboard order, and compare to the (sorted) input.
pub fn roundtrip_user_index(input: &UserIndexInput) -> RoundTrip {
    // Build + save via the SAME endpoint the backfill emits from.
    let bytes = match project_user_index(input) {
        Ok(b) => b,
        Err(e) => return RoundTrip::flag(e),
    };
    let reloaded = match AutoCommit::load(&bytes) {
        Ok(d) => d,
        Err(e) => return RoundTrip::flag(format!("user doc did not reload: {e}")),
    };

    let mut expected: Vec<EntryNorm> = input
        .books
        .iter()
        .map(|(id, title, cover_ref, updated_at)| EntryNorm {
            id: id.clone(),
            title: title.clone(),
            cover_ref: cover_ref.clone(),
            updated_at: updated_at.clone(),
        })
        .collect();
    sort_entries(&mut expected);

    let mut actual = read_entries(&reloaded);
    sort_entries(&mut actual);

    if expected == actual {
        RoundTrip::Clean
    } else {
        RoundTrip::flag("user index differs (book entries / order)".to_string())
    }
}

/// Project a `user:` index to its canonical Automerge **snapshot bytes** — the
/// migration backfill's emit endpoint, sharing the build with the
/// [`roundtrip_user_index`] validator. Construction never fails, so this is always
/// `Ok`; the `Result` mirrors the other `project_*` endpoints for a uniform call site.
///
/// NOTE: user indices are DEFERRED in the lock-free backfill (ownership — which user
/// owns which book — lives in rhypedb, which the running server holds locked). This
/// endpoint exists so a later ownership-aware pass reuses the same projection.
pub fn project_user_index(input: &UserIndexInput) -> Result<Vec<u8>, String> {
    let mut doc = AutoCommit::new();
    let books_obj = doc.put_object(ROOT, "books", ObjType::Map).unwrap();
    for (id, title, cover_ref, updated_at) in &input.books {
        let entry = doc.put_object(&books_obj, id.as_str(), ObjType::Map).unwrap();
        let _ = doc.put(&entry, "title", title.as_str());
        if let Some(c) = cover_ref {
            let _ = doc.put(&entry, "cover_ref", c.as_str());
        }
        let _ = doc.put(&entry, "updated_at", updated_at.as_str());
    }
    Ok(doc.save())
}

fn read_entries(doc: &AutoCommit) -> Vec<EntryNorm> {
    let Some(books_obj) = get_obj(doc, &ROOT, "books") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for id in doc.keys(&books_obj) {
        let Some(entry) = get_obj(doc, &books_obj, id.as_str()) else {
            continue;
        };
        out.push(EntryNorm {
            title: get_str(doc, &entry, "title").unwrap_or_default(),
            cover_ref: get_str(doc, &entry, "cover_ref"),
            updated_at: get_str(doc, &entry, "updated_at").unwrap_or_default(),
            id,
        });
    }
    out
}

fn get_obj(doc: &AutoCommit, parent: &ObjId, prop: &str) -> Option<ObjId> {
    match doc.get(parent, prop) {
        Ok(Some((v, id))) if v.is_object() => Some(id),
        _ => None,
    }
}

fn get_str(doc: &AutoCommit, parent: &ObjId, prop: &str) -> Option<String> {
    doc.get(parent, prop)
        .ok()
        .flatten()
        .and_then(|(v, _)| v.to_str().map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_index_round_trips_clean() {
        let input = UserIndexInput {
            books: vec![
                ("b1".into(), "Moon Over Water".into(), Some("cover-a".into()), "2026-02-01 10:00:00".into()),
                ("b2".into(), "The Long Road".into(), None, "2026-03-15 09:00:00".into()),
                ("b3".into(), "Winter Harbour".into(), None, "2026-05-20 12:00:00".into()),
            ],
        };
        assert_eq!(roundtrip_user_index(&input), RoundTrip::Clean);
    }

    #[test]
    fn empty_user_index_is_clean() {
        assert_eq!(
            roundtrip_user_index(&UserIndexInput { books: vec![] }),
            RoundTrip::Clean
        );
    }
}
