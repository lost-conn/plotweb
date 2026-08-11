//! Book-structure round-trip (`book:`) — schema §2.
//!
//! Builds the `book:` Automerge doc (meta · chapter order + titles · notes tree +
//! titles/colors) from server-side inputs, `save()`/`load()`s it, projects it back,
//! and asserts the normalized structure equals the input. The Automerge construction
//! is a deliberate mirror of `plotweb-web/src/local_book.rs::build_doc` /
//! `project_*`, so a server-migrated `book:` doc has the same shape as a
//! client-written one.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ROOT, ReadDoc};

use plotweb_common::FontSettings;

use crate::RoundTrip;

/// Raw inputs for a `book:` structure doc — the fields the git store holds, adapted
/// off `BookData` / `ChapterData` / `NotesTreeJson` / `NoteData` by the caller.
#[derive(Clone, Debug)]
pub struct BookStructureInput {
    pub title: String,
    pub description: String,
    /// Whole-value LWW font settings (schema §2 v1). Serialized the same way the
    /// client does: `serde_json::to_string(&font_settings.unwrap_or_default())`.
    pub font_settings: Option<FontSettings>,
    pub cover_ref: Option<String>,
    pub created_at: String,
    /// `(chapter_id, title)` in **authoritative order** (`book.json` `chapter_order`).
    pub chapters: Vec<(String, String)>,
    /// Notes tree: root order.
    pub root_order: Vec<String>,
    /// Notes tree: `parent -> ordered child ids` (empty lists are dropped, matching
    /// the client projection).
    pub children: HashMap<String, Vec<String>>,
    /// Notes tree: collapsed note ids.
    pub collapsed: Vec<String>,
    /// `(note_id, title, color?)` for every note in the book.
    pub notes: Vec<(String, String, Option<String>)>,
}

/// The normalized, comparable view of a book structure. `List`s (chapter order, note
/// root/child order) keep order; `Map`s compare as maps regardless of Automerge key
/// order.
#[derive(Debug, PartialEq, Eq)]
struct BookNorm {
    title: String,
    description: String,
    font_settings_json: String,
    cover_ref: Option<String>,
    created_at: String,
    chapters: Vec<(String, String)>,
    root_order: Vec<String>,
    children: BTreeMap<String, Vec<String>>,
    collapsed: BTreeSet<String>,
    titles: BTreeMap<String, String>,
    colors: BTreeMap<String, String>,
}

fn font_settings_json(fs: &Option<FontSettings>) -> String {
    serde_json::to_string(&fs.clone().unwrap_or_default()).unwrap_or_else(|_| "{}".to_string())
}

impl BookStructureInput {
    /// The structure we EXPECT to read back, with the same filtering the projection
    /// applies (empty child lists dropped; colors only for notes that have one).
    fn expected(&self) -> BookNorm {
        let mut children = BTreeMap::new();
        for (parent, kids) in &self.children {
            if !kids.is_empty() {
                children.insert(parent.clone(), kids.clone());
            }
        }
        let mut titles = BTreeMap::new();
        let mut colors = BTreeMap::new();
        for (id, title, color) in &self.notes {
            titles.insert(id.clone(), title.clone());
            if let Some(c) = color {
                colors.insert(id.clone(), c.clone());
            }
        }
        BookNorm {
            title: self.title.clone(),
            description: self.description.clone(),
            font_settings_json: font_settings_json(&self.font_settings),
            cover_ref: self.cover_ref.clone(),
            created_at: self.created_at.clone(),
            chapters: self.chapters.clone(),
            root_order: self.root_order.clone(),
            children,
            collapsed: self.collapsed.iter().cloned().collect(),
            titles,
            colors,
        }
    }
}

/// Round-trip a `book:` structure: build the Automerge doc, persist + reload it,
/// project it back, and compare to the input. Any mismatch is a [`RoundTrip::Flagged`]
/// naming the field that diverged.
pub fn roundtrip_book_structure(input: &BookStructureInput) -> RoundTrip {
    // Build + save via the SAME endpoint the backfill emits from, so a validated
    // `book:` structure is exactly the bytes a blob would hold.
    let bytes = match project_book_structure(input) {
        Ok(b) => b,
        Err(e) => return RoundTrip::flag(e),
    };
    let reloaded = match AutoCommit::load(&bytes) {
        Ok(d) => d,
        Err(e) => return RoundTrip::flag(format!("book doc did not reload: {e}")),
    };

    let expected = input.expected();
    let actual = read_book_norm(&reloaded);

    if expected == actual {
        RoundTrip::Clean
    } else {
        RoundTrip::flag(describe_book_diff(&expected, &actual))
    }
}

/// Project a `book:` structure to its canonical Automerge **snapshot bytes** — the
/// migration backfill's emit endpoint, sharing [`build_book_doc`] with the
/// [`roundtrip_book_structure`] validator so a backfilled blob is exactly what the
/// audit certified. Construction never fails, so this is always `Ok`; the `Result`
/// mirrors [`project_body`](crate::body::project_body) for a uniform call site.
pub fn project_book_structure(input: &BookStructureInput) -> Result<Vec<u8>, String> {
    let mut doc = AutoCommit::new();
    build_book_doc(&mut doc, input);
    Ok(doc.save())
}

// ── Construction (mirror of local_book::build_doc) ───────────────────────────

fn build_book_doc(doc: &mut AutoCommit, input: &BookStructureInput) {
    // meta
    let meta = doc.put_object(ROOT, "meta", ObjType::Map).unwrap();
    let _ = doc.put(&meta, "title", input.title.as_str());
    let _ = doc.put(&meta, "description", input.description.as_str());
    let _ = doc.put(&meta, "font_settings", font_settings_json(&input.font_settings));
    if let Some(cover) = &input.cover_ref {
        let _ = doc.put(&meta, "cover_ref", cover.as_str());
    }
    let _ = doc.put(&meta, "created_at", input.created_at.as_str());

    // chapters (order List + titles Map)
    let chs = doc.put_object(ROOT, "chapters", ObjType::List).unwrap();
    let ctitles = doc.put_object(ROOT, "chapter_titles", ObjType::Map).unwrap();
    for (i, (id, title)) in input.chapters.iter().enumerate() {
        let _ = doc.insert(&chs, i, id.as_str());
        let _ = doc.put(&ctitles, id.as_str(), title.as_str());
    }

    // notes
    let notes_obj = doc.put_object(ROOT, "notes", ObjType::Map).unwrap();

    let root = doc.put_object(&notes_obj, "root_order", ObjType::List).unwrap();
    for (i, id) in input.root_order.iter().enumerate() {
        let _ = doc.insert(&root, i, id.as_str());
    }

    let children = doc.put_object(&notes_obj, "children", ObjType::Map).unwrap();
    for (parent, kids) in &input.children {
        if kids.is_empty() {
            continue;
        }
        let list = doc.put_object(&children, parent.as_str(), ObjType::List).unwrap();
        for (i, k) in kids.iter().enumerate() {
            let _ = doc.insert(&list, i, k.as_str());
        }
    }

    let collapsed = doc.put_object(&notes_obj, "collapsed", ObjType::Map).unwrap();
    for id in &input.collapsed {
        let _ = doc.put(&collapsed, id.as_str(), true);
    }

    let titles = doc.put_object(&notes_obj, "titles", ObjType::Map).unwrap();
    let colors = doc.put_object(&notes_obj, "colors", ObjType::Map).unwrap();
    for (id, title, color) in &input.notes {
        let _ = doc.put(&titles, id.as_str(), title.as_str());
        if let Some(c) = color {
            let _ = doc.put(&colors, id.as_str(), c.as_str());
        }
    }
}

// ── Read-back (mirror of local_book::project_*) ──────────────────────────────

fn read_book_norm(doc: &AutoCommit) -> BookNorm {
    let meta = get_obj(doc, &ROOT, "meta");
    let title = meta.as_ref().and_then(|m| get_str(doc, m, "title")).unwrap_or_default();
    let description = meta
        .as_ref()
        .and_then(|m| get_str(doc, m, "description"))
        .unwrap_or_default();
    let font_settings_json = meta
        .as_ref()
        .and_then(|m| get_str(doc, m, "font_settings"))
        .unwrap_or_default();
    let cover_ref = meta.as_ref().and_then(|m| get_str(doc, m, "cover_ref"));
    let created_at = meta
        .as_ref()
        .and_then(|m| get_str(doc, m, "created_at"))
        .unwrap_or_default();

    let order = get_obj(doc, &ROOT, "chapters")
        .map(|o| read_list_strings(doc, &o))
        .unwrap_or_default();
    let ctitles = get_obj(doc, &ROOT, "chapter_titles")
        .map(|o| read_map_strings(doc, &o))
        .unwrap_or_default();
    let chapters = order
        .into_iter()
        .map(|id| {
            let t = ctitles.get(&id).cloned().unwrap_or_default();
            (id, t)
        })
        .collect();

    let notes_obj = get_obj(doc, &ROOT, "notes");
    let root_order = notes_obj
        .as_ref()
        .and_then(|n| get_obj(doc, n, "root_order"))
        .map(|o| read_list_strings(doc, &o))
        .unwrap_or_default();

    let mut children = BTreeMap::new();
    if let Some(children_obj) = notes_obj.as_ref().and_then(|n| get_obj(doc, n, "children")) {
        for key in doc.keys(&children_obj) {
            if let Some(list) = get_obj(doc, &children_obj, key.as_str()) {
                let kids = read_list_strings(doc, &list);
                if !kids.is_empty() {
                    children.insert(key, kids);
                }
            }
        }
    }

    let mut collapsed = BTreeSet::new();
    if let Some(collapsed_obj) = notes_obj.as_ref().and_then(|n| get_obj(doc, n, "collapsed")) {
        for key in doc.keys(&collapsed_obj) {
            if doc
                .get(&collapsed_obj, key.as_str())
                .ok()
                .flatten()
                .and_then(|(v, _)| v.to_bool())
                .unwrap_or(false)
            {
                collapsed.insert(key);
            }
        }
    }

    let titles = notes_obj
        .as_ref()
        .and_then(|n| get_obj(doc, n, "titles"))
        .map(|o| read_map_strings_sorted(doc, &o))
        .unwrap_or_default();
    let colors = notes_obj
        .as_ref()
        .and_then(|n| get_obj(doc, n, "colors"))
        .map(|o| read_map_strings_sorted(doc, &o))
        .unwrap_or_default();

    BookNorm {
        title,
        description,
        font_settings_json,
        cover_ref,
        created_at,
        chapters,
        root_order,
        children,
        collapsed,
        titles,
        colors,
    }
}

fn describe_book_diff(expected: &BookNorm, actual: &BookNorm) -> String {
    let mut parts = Vec::new();
    if expected.title != actual.title {
        parts.push("meta.title".to_string());
    }
    if expected.description != actual.description {
        parts.push("meta.description".to_string());
    }
    if expected.font_settings_json != actual.font_settings_json {
        parts.push("meta.font_settings".to_string());
    }
    if expected.cover_ref != actual.cover_ref {
        parts.push("meta.cover_ref".to_string());
    }
    if expected.created_at != actual.created_at {
        parts.push("meta.created_at".to_string());
    }
    if expected.chapters != actual.chapters {
        parts.push("chapters (order/titles)".to_string());
    }
    if expected.root_order != actual.root_order {
        parts.push("notes.root_order".to_string());
    }
    if expected.children != actual.children {
        parts.push("notes.children".to_string());
    }
    if expected.collapsed != actual.collapsed {
        parts.push("notes.collapsed".to_string());
    }
    if expected.titles != actual.titles {
        parts.push("notes.titles".to_string());
    }
    if expected.colors != actual.colors {
        parts.push("notes.colors".to_string());
    }
    format!("book structure differs at: {}", parts.join(", "))
}

// ── Automerge helpers (mirror of local_book's) ───────────────────────────────

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

fn read_list_strings(doc: &AutoCommit, obj: &ObjId) -> Vec<String> {
    let len = doc.length(obj);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        if let Ok(Some((v, _))) = doc.get(obj, i)
            && let Some(s) = v.to_str()
        {
            out.push(s.to_string());
        }
    }
    out
}

fn read_map_strings(doc: &AutoCommit, obj: &ObjId) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for key in doc.keys(obj) {
        if let Ok(Some((v, _))) = doc.get(obj, key.as_str())
            && let Some(s) = v.to_str()
        {
            out.insert(key, s.to_string());
        }
    }
    out
}

fn read_map_strings_sorted(doc: &AutoCommit, obj: &ObjId) -> BTreeMap<String, String> {
    read_map_strings(doc, obj).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BookStructureInput {
        BookStructureInput {
            title: "The Book".into(),
            description: "desc".into(),
            font_settings: None,
            cover_ref: Some("cover-hash".into()),
            created_at: "2026-01-01 00:00:00".into(),
            chapters: vec![
                ("c1".into(), "Opening".into()),
                ("c2".into(), "The Storm".into()),
            ],
            // n1 has child n2; n3 is a second root. n1 collapsed.
            root_order: vec!["n1".into(), "n3".into()],
            children: HashMap::from([("n1".to_string(), vec!["n2".to_string()])]),
            collapsed: vec!["n1".into()],
            notes: vec![
                ("n1".into(), "Characters".into(), Some("teal".into())),
                ("n2".into(), "Alice".into(), Some("red".into())),
                ("n3".into(), "Places".into(), None),
            ],
        }
    }

    #[test]
    fn book_structure_round_trips_clean() {
        assert_eq!(roundtrip_book_structure(&sample()), RoundTrip::Clean);
    }

    #[test]
    fn empty_book_round_trips_clean() {
        let input = BookStructureInput {
            title: "Empty".into(),
            description: String::new(),
            font_settings: None,
            cover_ref: None,
            created_at: "2026-01-01 00:00:00".into(),
            chapters: vec![],
            root_order: vec![],
            children: HashMap::new(),
            collapsed: vec![],
            notes: vec![],
        };
        assert_eq!(roundtrip_book_structure(&input), RoundTrip::Clean);
    }

    #[test]
    fn font_settings_whole_value_round_trips() {
        let mut input = sample();
        input.font_settings = Some(FontSettings {
            h1: Some("Playfair".into()),
            body: Some("Inter".into()),
            paragraph_spacing: Some(1.5),
            ..Default::default()
        });
        assert_eq!(roundtrip_book_structure(&input), RoundTrip::Clean);
    }
}

/// Compare the canonical bytes the server holds for a `book:` structure against what
/// git currently says.
///
/// The structure counterpart to [`compare_body`](crate::body::compare_body), and the
/// one likelier to move: chapter order, titles and the notes tree change through
/// ordinary use, and the client writes them to its `book:` document *and* to REST. A
/// divergence here means one of those two writes went missing.
pub fn compare_book_structure(input: &BookStructureInput, canonical: &[u8]) -> crate::Shadow {
    let reloaded = match AutoCommit::load(canonical) {
        Ok(d) => d,
        Err(e) => {
            return crate::Shadow::Unreadable {
                reason: format!("stored book document did not load: {e}"),
            }
        }
    };
    let expected = input.expected();
    let actual = read_book_norm(&reloaded);
    if expected == actual {
        crate::Shadow::Match
    } else {
        crate::Shadow::Diverged {
            detail: describe_book_diff(&expected, &actual),
        }
    }
}
