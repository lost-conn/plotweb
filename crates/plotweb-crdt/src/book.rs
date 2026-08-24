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
///
/// This is both the shadow pass's comparison view and what a cut-over read serves, so
/// the two can never drift apart: whatever the reader hands the frontend is exactly
/// what the validator checked against git.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookStructure {
    pub title: String,
    pub description: String,
    /// Font settings as the stored JSON string (whole-value LWW, schema §2 v1).
    pub font_settings_json: String,
    pub cover_ref: Option<String>,
    pub created_at: String,
    /// `(chapter_id, title)` in authoritative order.
    pub chapters: Vec<(String, String)>,
    pub root_order: Vec<String>,
    pub children: BTreeMap<String, Vec<String>>,
    pub collapsed: BTreeSet<String>,
    pub note_titles: BTreeMap<String, String>,
    pub note_colors: BTreeMap<String, String>,
}

fn font_settings_json(fs: &Option<FontSettings>) -> String {
    serde_json::to_string(&fs.clone().unwrap_or_default()).unwrap_or_else(|_| "{}".to_string())
}

impl BookStructureInput {
    /// The structure we EXPECT to read back, with the same filtering the projection
    /// applies (empty child lists dropped; colors only for notes that have one).
    pub fn structure(&self) -> BookStructure {
        let mut children = BTreeMap::new();
        for (parent, kids) in &self.children {
            if !kids.is_empty() {
                children.insert(parent.clone(), kids.clone());
            }
        }
        let mut note_titles = BTreeMap::new();
        let mut note_colors = BTreeMap::new();
        for (id, title, color) in &self.notes {
            note_titles.insert(id.clone(), title.clone());
            if let Some(c) = color {
                note_colors.insert(id.clone(), c.clone());
            }
        }
        BookStructure {
            title: self.title.clone(),
            description: self.description.clone(),
            font_settings_json: font_settings_json(&self.font_settings),
            cover_ref: self.cover_ref.clone(),
            created_at: self.created_at.clone(),
            chapters: self.chapters.clone(),
            root_order: self.root_order.clone(),
            children,
            collapsed: self.collapsed.iter().cloned().collect(),
            note_titles,
            note_colors,
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

    let expected = input.structure();
    let actual = read_book_structure(&reloaded);

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

/// Read a stored `book:` document back into the structure the API serves.
///
/// The counterpart to [`project_book_structure`], and the structure half of cutover:
/// once a book reads from the canonical store, this is where its chapter order, titles
/// and notes tree come from. It is the same read-back the shadow pass compares with, so
/// a document the validator called clean is a document this returns git's answer for.
pub fn materialize_book_structure(canonical: &[u8]) -> Result<BookStructure, String> {
    let doc =
        AutoCommit::load(canonical).map_err(|e| format!("book document did not load: {e}"))?;
    Ok(read_book_structure(&doc))
}

/// Record `input` into an existing canonical `book:` document **as an edit**, returning
/// the updated bytes.
///
/// The structure counterpart of [`apply_content`](crate::body::apply_content), and it
/// exists for the same reason: a REST write to a cut-over book has to land *inside* the
/// canonical document, descending from what synced devices already hold. Re-projecting
/// from git instead would produce a document sharing no history with theirs, and the
/// first merge would concatenate the two rather than reconcile them (§D8).
///
/// Only what actually differs is written, so a save that renames one chapter is one
/// small change rather than a rewrite of the whole structure — which matters when two
/// devices are editing different parts of the same book at once.
///
/// # Why removals are explicit
///
/// `input` is read out of git, and git lags the canonical document: a chapter created on
/// a device is absent from git until the mirror commits it. Treating "absent from
/// `input`" as "deleted" would therefore make an unrelated rename destroy that chapter
/// — the two cases are indistinguishable from git alone.
///
/// So a chapter or note the canonical copy has and `input` does not is **kept**, in the
/// position it already occupies, unless its id appears in `removable`. Callers name what
/// they actually deleted; nothing else can be lost by a write that meant to do something
/// else.
pub fn apply_book_structure(
    canonical: &[u8],
    input: &BookStructureInput,
    removable: &[String],
) -> Result<Vec<u8>, String> {
    let mut doc =
        AutoCommit::load(canonical).map_err(|e| format!("book document did not load: {e}"))?;
    let current = read_book_structure(&doc);
    let want = keeping_unmirrored(&current, input.structure(), removable);
    // `current` is deduped, so a document holding a repeated id looks identical to a
    // clean one here — and returning early would leave the repeat in place forever.
    // Checking the raw lists is what makes writing the structure *repair* the document
    // rather than merely stop showing the damage.
    if current == want && !holds_a_repeated_id(&doc) {
        return Ok(canonical.to_vec());
    }

    let meta = ensure_obj(&mut doc, &ROOT, "meta", ObjType::Map)?;
    if current.title != want.title {
        let _ = doc.put(&meta, "title", want.title.as_str());
    }
    if current.description != want.description {
        let _ = doc.put(&meta, "description", want.description.as_str());
    }
    if current.font_settings_json != want.font_settings_json {
        let _ = doc.put(&meta, "font_settings", want.font_settings_json.as_str());
    }
    if current.cover_ref != want.cover_ref {
        match &want.cover_ref {
            Some(cover) => {
                let _ = doc.put(&meta, "cover_ref", cover.as_str());
            }
            None => {
                let _ = doc.delete(&meta, "cover_ref");
            }
        }
    }
    if current.created_at != want.created_at {
        let _ = doc.put(&meta, "created_at", want.created_at.as_str());
    }

    // Chapters: order in a list, titles in a map beside it.
    let chapter_ids: Vec<String> = want.chapters.iter().map(|(id, _)| id.clone()).collect();
    let chapters_obj = ensure_obj(&mut doc, &ROOT, "chapters", ObjType::List)?;
    reconcile_list(&mut doc, &chapters_obj, &chapter_ids);
    let titles_obj = ensure_obj(&mut doc, &ROOT, "chapter_titles", ObjType::Map)?;
    let want_titles: BTreeMap<String, String> = want.chapters.iter().cloned().collect();
    let have_titles: BTreeMap<String, String> = current.chapters.iter().cloned().collect();
    reconcile_string_map(&mut doc, &titles_obj, &have_titles, &want_titles);

    let notes_obj = ensure_obj(&mut doc, &ROOT, "notes", ObjType::Map)?;

    let root_obj = ensure_obj(&mut doc, &notes_obj, "root_order", ObjType::List)?;
    reconcile_list(&mut doc, &root_obj, &want.root_order);

    let children_obj = ensure_obj(&mut doc, &notes_obj, "children", ObjType::Map)?;
    for parent in current.children.keys() {
        // An empty child list is *absent*, not an empty list — the projection drops it,
        // so leaving one behind would read back as a difference forever.
        if !want.children.contains_key(parent) {
            let _ = doc.delete(&children_obj, parent.as_str());
        }
    }
    for (parent, kids) in &want.children {
        let list = ensure_obj(&mut doc, &children_obj, parent.as_str(), ObjType::List)?;
        reconcile_list(&mut doc, &list, kids);
    }

    let collapsed_obj = ensure_obj(&mut doc, &notes_obj, "collapsed", ObjType::Map)?;
    for id in current.collapsed.difference(&want.collapsed) {
        let _ = doc.delete(&collapsed_obj, id.as_str());
    }
    for id in want.collapsed.difference(&current.collapsed) {
        let _ = doc.put(&collapsed_obj, id.as_str(), true);
    }

    let note_titles_obj = ensure_obj(&mut doc, &notes_obj, "titles", ObjType::Map)?;
    reconcile_string_map(
        &mut doc,
        &note_titles_obj,
        &current.note_titles,
        &want.note_titles,
    );
    let note_colors_obj = ensure_obj(&mut doc, &notes_obj, "colors", ObjType::Map)?;
    reconcile_string_map(
        &mut doc,
        &note_colors_obj,
        &current.note_colors,
        &want.note_colors,
    );

    Ok(doc.save())
}

/// Whether any list in the document names the same id twice — see
/// [`read_list_strings`] for how that happens.
fn holds_a_repeated_id(doc: &AutoCommit) -> bool {
    let repeated = |obj: &ObjId| {
        let raw = read_list_strings_raw(doc, obj);
        let mut seen = BTreeSet::new();
        raw.iter().any(|id| !seen.insert(id.clone()))
    };

    if get_obj(doc, &ROOT, "chapters").is_some_and(|o| repeated(&o)) {
        return true;
    }
    let Some(notes) = get_obj(doc, &ROOT, "notes") else {
        return false;
    };
    if get_obj(doc, &notes, "root_order").is_some_and(|o| repeated(&o)) {
        return true;
    }
    if let Some(children) = get_obj(doc, &notes, "children") {
        for key in doc.keys(&children) {
            if get_obj(doc, &children, key.as_str()).is_some_and(|o| repeated(&o)) {
                return true;
            }
        }
    }
    false
}

/// Put back anything the canonical copy has that `want` does not mention and the caller
/// did not say it removed — see [`apply_book_structure`]'s note on why absence from git
/// is not evidence of deletion.
///
/// A kept chapter goes back at the index it already had, so a device's new chapter three
/// stays chapter three rather than being shuffled to the end by an unrelated rename.
fn keeping_unmirrored(
    current: &BookStructure,
    mut want: BookStructure,
    removable: &[String],
) -> BookStructure {
    let removing = |id: &String| removable.iter().any(|r| r == id);

    for (i, (id, title)) in current.chapters.iter().enumerate() {
        if removing(id) || want.chapters.iter().any(|(wid, _)| wid == id) {
            continue;
        }
        let at = i.min(want.chapters.len());
        want.chapters.insert(at, (id.clone(), title.clone()));
    }

    for (id, title) in &current.note_titles {
        if removing(id) || want.note_titles.contains_key(id) {
            continue;
        }
        want.note_titles.insert(id.clone(), title.clone());
        if let Some(color) = current.note_colors.get(id) {
            want.note_colors.insert(id.clone(), color.clone());
        }
        // Its place in the tree comes back with it; a note present but unreachable
        // would be invisible in the sidebar, which is indistinguishable from lost.
        if current.root_order.contains(id) && !want.root_order.contains(id) {
            let at = current
                .root_order
                .iter()
                .position(|r| r == id)
                .unwrap_or(want.root_order.len())
                .min(want.root_order.len());
            want.root_order.insert(at, id.clone());
        }
        for (parent, kids) in &current.children {
            if kids.contains(id) && !want.children.get(parent).is_some_and(|k| k.contains(id)) {
                want.children.entry(parent.clone()).or_default().push(id.clone());
            }
        }
    }
    want
}

/// Bring a list to `want` with as few edits as possible.
///
/// Not "delete everything and re-insert": that would be a change touching every element,
/// and two devices doing it concurrently merge into a list holding both copies. Removing
/// what left and moving only what actually moved keeps a rename or a single reorder
/// small enough to merge cleanly with an edit elsewhere in the same list.
fn reconcile_list(doc: &mut AutoCommit, obj: &ObjId, want: &[String]) {
    // Raw, duplicates included: this is the write that repairs them.
    let mut have = read_list_strings_raw(doc, obj);
    let wanted: BTreeSet<&String> = want.iter().collect();

    for i in (0..have.len()).rev() {
        if !wanted.contains(&have[i]) {
            let _ = doc.delete(obj, i);
            have.remove(i);
        }
    }
    for (i, id) in want.iter().enumerate() {
        if have.get(i) == Some(id) {
            continue;
        }
        if let Some(pos) = have.iter().position(|h| h == id) {
            let _ = doc.delete(obj, pos);
            have.remove(pos);
        }
        let _ = doc.insert(obj, i, id.as_str());
        have.insert(i, id.clone());
    }
    // Anything past the target is a leftover duplicate — the loop above positions each
    // wanted id once and stops. Without this the repeat simply stays, and writing the
    // list would never repair a document a merge had already doubled up.
    while have.len() > want.len() {
        let _ = doc.delete(obj, have.len() - 1);
        have.pop();
    }
}

fn reconcile_string_map(
    doc: &mut AutoCommit,
    obj: &ObjId,
    have: &BTreeMap<String, String>,
    want: &BTreeMap<String, String>,
) {
    for key in have.keys() {
        if !want.contains_key(key) {
            let _ = doc.delete(obj, key.as_str());
        }
    }
    for (key, value) in want {
        if have.get(key) != Some(value) {
            let _ = doc.put(obj, key.as_str(), value.as_str());
        }
    }
}

/// The object at `prop`, creating it if the document has none.
///
/// A document written by an older client can be missing a whole section (notes, say),
/// and an apply that assumed otherwise would silently drop the write.
fn ensure_obj(
    doc: &mut AutoCommit,
    parent: &ObjId,
    prop: &str,
    kind: ObjType,
) -> Result<ObjId, String> {
    if let Some(id) = get_obj(doc, parent, prop) {
        return Ok(id);
    }
    doc.put_object(parent, prop, kind)
        .map_err(|e| format!("could not create {prop}: {e}"))
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

fn read_book_structure(doc: &AutoCommit) -> BookStructure {
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

    let note_titles = notes_obj
        .as_ref()
        .and_then(|n| get_obj(doc, n, "titles"))
        .map(|o| read_map_strings_sorted(doc, &o))
        .unwrap_or_default();
    let note_colors = notes_obj
        .as_ref()
        .and_then(|n| get_obj(doc, n, "colors"))
        .map(|o| read_map_strings_sorted(doc, &o))
        .unwrap_or_default();

    BookStructure {
        title,
        description,
        font_settings_json,
        cover_ref,
        created_at,
        chapters,
        root_order,
        children,
        collapsed,
        note_titles,
        note_colors,
    }
}

fn describe_book_diff(expected: &BookStructure, actual: &BookStructure) -> String {
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
    if expected.note_titles != actual.note_titles {
        parts.push("notes.titles".to_string());
    }
    if expected.note_colors != actual.note_colors {
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

/// Every id in a `List`, with repeats collapsed.
///
/// A chapter cannot be in two places at once, so a repeat carries no meaning — but
/// Automerge will hold one happily. Two writers inserting the same id is enough: a
/// browser's dual-write into its local document and the server's apply of the same REST
/// change into the canonical one are concurrent insertions of equal values, and the
/// merge keeps both. Collapsing on read means a document that already has one cannot
/// show it to anybody; `reconcile_list` removes it on the next write.
fn read_list_strings(doc: &AutoCommit, obj: &ObjId) -> Vec<String> {
    read_list_strings_raw(doc, obj)
        .into_iter()
        .fold(Vec::new(), |mut acc, s| {
            if !acc.contains(&s) {
                acc.push(s);
            }
            acc
        })
}

fn read_list_strings_raw(doc: &AutoCommit, obj: &ObjId) -> Vec<String> {
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

    /// What a peer device holding the canonical document ends up with after the
    /// server's edit reaches it. Merging rather than reloading is the point: it proves
    /// the applied write is a *change* to the shared history, not a new document.
    fn merged_with(original: &[u8], applied: &[u8]) -> BookStructure {
        let mut peer = AutoCommit::load(original).expect("peer loads");
        let mut server = AutoCommit::load(applied).expect("server loads");
        peer.merge(&mut server).expect("merge");
        read_book_structure(&peer)
    }

    #[test]
    fn a_stored_structure_materializes_back_to_what_was_projected() {
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let got = materialize_book_structure(&bytes).expect("materialize");
        assert_eq!(got, input.structure());
    }

    #[test]
    fn applying_an_unchanged_structure_writes_nothing() {
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let again = apply_book_structure(&bytes, &input, &[]).expect("apply");
        assert_eq!(
            again, bytes,
            "an idle save must not add a change — every one of those is a sync round \
             and a mirror commit for nobody"
        );
    }

    #[test]
    fn a_renamed_chapter_reaches_a_device_holding_the_document() {
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let mut renamed = input.clone();
        renamed.chapters[1].1 = "The Squall".into();

        let applied = apply_book_structure(&bytes, &renamed, &[]).expect("apply");
        assert_eq!(merged_with(&bytes, &applied), renamed.structure());
    }

    #[test]
    fn a_reorder_moves_chapters_rather_than_duplicating_them() {
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let mut reordered = input.clone();
        reordered.chapters.reverse();

        let applied = apply_book_structure(&bytes, &reordered, &[]).expect("apply");
        let merged = merged_with(&bytes, &applied);
        assert_eq!(merged.chapters, reordered.structure().chapters);
        assert_eq!(
            merged.chapters.len(),
            2,
            "a rebuilt list would merge into one holding both orders"
        );
    }

    #[test]
    fn adding_and_removing_chapters_is_carried_through() {
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let mut changed = input.clone();
        changed.chapters.remove(0);
        changed.chapters.push(("c3".into(), "Aftermath".into()));

        let applied =
            apply_book_structure(&bytes, &changed, &["c1".to_string()]).expect("apply");
        let merged = merged_with(&bytes, &applied);
        assert_eq!(
            merged.chapters,
            vec![
                ("c2".to_string(), "The Storm".to_string()),
                ("c3".to_string(), "Aftermath".to_string())
            ]
        );
        assert_eq!(merged, changed.structure(), "including the titles map");
    }

    #[test]
    fn the_notes_tree_follows_too() {
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let mut changed = input.clone();
        // n2 moves out from under n1 to the root; n1 is expanded; n3 loses its title.
        changed.children.clear();
        changed.root_order = vec!["n1".into(), "n2".into(), "n3".into()];
        changed.collapsed.clear();
        changed.notes[2].1 = "Settings".into();
        changed.notes[1].2 = None;

        let applied = apply_book_structure(&bytes, &changed, &[]).expect("apply");
        let merged = merged_with(&bytes, &applied);
        assert_eq!(merged, changed.structure());
        assert!(
            merged.children.is_empty(),
            "an emptied child list is absent, not an empty list"
        );
        assert!(
            !merged.note_colors.contains_key("n2"),
            "a cleared colour must be removed, not left at its old value"
        );
    }

    #[test]
    fn meta_changes_including_a_cleared_cover_are_carried_through() {
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let mut changed = input.clone();
        changed.title = "The Book, Revised".into();
        changed.description = "a better description".into();
        changed.cover_ref = None;

        let applied = apply_book_structure(&bytes, &changed, &[]).expect("apply");
        assert_eq!(merged_with(&bytes, &applied), changed.structure());
    }

    #[test]
    fn an_edit_elsewhere_in_the_book_survives_the_apply() {
        // The property that decides whether cutover is safe with two devices open: a
        // server-applied write must merge with a device's concurrent change, not
        // clobber it. Here the device renames chapter one while the server reorders.
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");

        let mut device = AutoCommit::load(&bytes).expect("device loads");
        let ctitles = get_obj(&device, &ROOT, "chapter_titles").expect("titles");
        device.put(&ctitles, "c1", "Opening, revised").expect("device edit");
        let device_bytes = device.save();

        let mut reordered = input.clone();
        reordered.chapters.reverse();
        let applied = apply_book_structure(&bytes, &reordered, &[]).expect("apply");

        let mut merged = AutoCommit::load(&applied).expect("load");
        let mut theirs = AutoCommit::load(&device_bytes).expect("load");
        merged.merge(&mut theirs).expect("merge");
        let structure = read_book_structure(&merged);

        assert_eq!(
            structure.chapters,
            vec![
                ("c2".to_string(), "The Storm".to_string()),
                ("c1".to_string(), "Opening, revised".to_string())
            ],
            "the server's reorder and the device's rename must both survive"
        );
    }

    #[test]
    fn a_document_missing_a_section_gains_it_rather_than_dropping_the_write() {
        // A `book:` doc written before a section existed (or by an older client): the
        // apply has to create it, or the write silently disappears.
        let mut bare = AutoCommit::new();
        let meta = bare.put_object(ROOT, "meta", ObjType::Map).unwrap();
        bare.put(&meta, "title", "The Book").unwrap();
        let bytes = bare.save();

        let applied = apply_book_structure(&bytes, &sample(), &[]).expect("apply");
        assert_eq!(
            materialize_book_structure(&applied).expect("materialize"),
            sample().structure()
        );
    }

    #[test]
    fn a_doubled_chapter_list_reads_as_one_of_each() {
        // What two writers inserting the same id produces. A merge keeps both copies,
        // and the author sees every chapter twice — reported from production, with the
        // second copy rendering empty because the projection had already consumed the
        // id.
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let mut doc = AutoCommit::load(&bytes).expect("load");
        let chs = get_obj(&doc, &ROOT, "chapters").expect("chapters");
        doc.insert(&chs, 2, "c1").expect("duplicate insert");
        doc.insert(&chs, 3, "c2").expect("duplicate insert");
        let doubled = doc.save();

        assert_eq!(
            materialize_book_structure(&doubled).expect("materialize").chapters,
            input.structure().chapters,
            "a doubled list must never reach a reader"
        );
    }

    #[test]
    fn writing_the_structure_repairs_a_doubled_list() {
        // Collapsing on read keeps it out of sight; this is what takes it out of the
        // document, so the damage does not outlive the bug that caused it.
        let input = sample();
        let bytes = project_book_structure(&input).expect("project");
        let mut doc = AutoCommit::load(&bytes).expect("load");
        let chs = get_obj(&doc, &ROOT, "chapters").expect("chapters");
        doc.insert(&chs, 2, "c1").expect("duplicate insert");
        let doubled = doc.save();

        let repaired = apply_book_structure(&doubled, &input, &[]).expect("apply");
        let raw = {
            let reloaded = AutoCommit::load(&repaired).expect("load");
            let obj = get_obj(&reloaded, &ROOT, "chapters").expect("chapters");
            read_list_strings_raw(&reloaded, &obj)
        };
        assert_eq!(
            raw,
            vec!["c1".to_string(), "c2".to_string()],
            "the duplicate must be gone from the document, not just hidden"
        );
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
    let expected = input.structure();
    let actual = read_book_structure(&reloaded);
    if expected == actual {
        crate::Shadow::Match
    } else {
        crate::Shadow::Diverged {
            detail: describe_book_diff(&expected, &actual),
        }
    }
}
