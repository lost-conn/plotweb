//! The editor plugin that paints the find panel's hits into the open chapter.
//!
//! [`super::matcher`] decides what a match is and [`super::doc`] finds them;
//! this is the half that draws. Like the spellchecker
//! ([`crate::spell::plugin`]) it implements rinch's
//! [`Plugin`](rinch_editor_core::Plugin) and contributes
//! [`Decoration::Inline`]s — one `pm-search-hit` per hit, plus a second
//! `pm-search-hit-current` over whichever one the author is standing on. rinch's
//! view merges overlapping decorations into one span carrying both classes, so
//! the current hit is a single element with `class="pm-search-hit
//! pm-search-hit-current"` and the CSS for the two is independent.
//!
//! # Recomputed from the document, never stored
//!
//! The obvious design — the panel computes the hits and hands the plugin a list
//! of ranges — is the one that goes wrong. `EditorState::decorations()` is
//! called afresh for whatever state is being rendered, including the state
//! *after* a replace; a stored list would be describing the document as it was
//! one transaction ago, and would paint highlights over text that has moved.
//!
//! So the shared state holds the **query**, not the answer, and the plugin runs
//! the search itself against the document it is decorating. That is the same
//! "recomputed, not remapped" rule the spellchecker follows, and it is why
//! replacing a hit makes its highlight disappear on its own.
//!
//! There is deliberately no block cache here, unlike the spellchecker's. A
//! misspelling costs a dictionary lookup per token; a hit costs a substring
//! scan, which is cheap enough that caching it would be more machinery than the
//! thing it saves.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rinch_editor_core::{Attrs, Decoration, DecorationSet, EditorState, Plugin, PluginKey};

use super::doc::find_in_doc;
use super::matcher::FindOptions;

/// The stable key for this plugin's slot in an editor state.
pub const SEARCH_KEY: PluginKey = PluginKey("plotweb-search");

/// The class on every hit in the open chapter.
pub const SEARCH_HIT_CLASS: &str = "pm-search-hit";

/// The extra class on the one hit Prev/Next is standing on.
pub const SEARCH_CURRENT_CLASS: &str = "pm-search-hit-current";

// ── Shared, mutable, outside the plugin ──────────────────────────────────────

/// What the find panel is asking for, shared with the plugin inside the editor
/// state.
///
/// Behind an `Rc` for the same reason the spellchecker's is: `EditorState` holds
/// its plugins as `Rc<dyn Plugin>` and hands out no way to reach back into one,
/// so the panel and the plugin need a third thing they both hold.
#[derive(Default)]
pub struct SearchShared {
    /// Whether the panel is open. A closed panel paints nothing, whatever is
    /// left in the query field.
    active: Cell<bool>,
    query: RefCell<String>,
    opts: Cell<FindOptions>,
    /// Which hit (by index, in document order) is the current one.
    current: Cell<Option<usize>>,
    /// Bumped whenever any of the above changes. The panel reads it to decide
    /// whether a cached result list is still the answer to the question being
    /// asked; nothing here caches on it, which is the point — see the module
    /// header.
    generation: Cell<u64>,
}

impl SearchShared {
    /// A shared state with the panel closed.
    pub fn new() -> Rc<SearchShared> {
        Rc::new(SearchShared::default())
    }

    /// Whether hits are being painted.
    pub fn active(&self) -> bool {
        self.active.get()
    }

    /// The query being painted (empty when there is none).
    pub fn query(&self) -> String {
        self.query.borrow().clone()
    }

    /// The switches the query is being matched under.
    pub fn options(&self) -> FindOptions {
        self.opts.get()
    }

    /// The current hit's index in document order, if any.
    pub fn current(&self) -> Option<usize> {
        self.current.get()
    }

    /// The generation counter — bumped by every setter below.
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Set the search being painted. Bumps the generation.
    pub fn set_search(&self, query: &str, opts: FindOptions) {
        *self.query.borrow_mut() = query.to_string();
        self.opts.set(opts);
        self.bump();
    }

    /// Open or close the painting. Bumps the generation.
    pub fn set_active(&self, active: bool) {
        self.active.set(active);
        self.bump();
    }

    /// Point at the hit Prev/Next has landed on, or nothing. Bumps the
    /// generation.
    pub fn set_current(&self, current: Option<usize>) {
        self.current.set(current);
        self.bump();
    }

    /// Stop painting and forget the query — what Escape does.
    pub fn clear(&self) {
        self.active.set(false);
        self.query.borrow_mut().clear();
        self.current.set(None);
        self.bump();
    }

    fn bump(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }
}

// ── The plugin ───────────────────────────────────────────────────────────────

/// Highlights the find panel's hits in a rinch editor.
pub struct SearchHighlightPlugin {
    shared: Rc<SearchShared>,
}

impl SearchHighlightPlugin {
    /// A plugin reading `shared`.
    pub fn new(shared: Rc<SearchShared>) -> SearchHighlightPlugin {
        SearchHighlightPlugin { shared }
    }

    /// The shared state this plugin reads.
    pub fn shared(&self) -> &Rc<SearchShared> {
        &self.shared
    }
}

impl Plugin for SearchHighlightPlugin {
    fn key(&self) -> PluginKey {
        SEARCH_KEY
    }

    fn decorations(&self, state: &EditorState) -> DecorationSet {
        if !self.shared.active() {
            return DecorationSet::empty();
        }
        let query = self.shared.query();
        if query.is_empty() {
            return DecorationSet::empty();
        }
        let hits = find_in_doc(&state.doc, &query, self.shared.options());
        let current = self.shared.current();
        let mut out = Vec::with_capacity(hits.len() + 1);
        for (i, hit) in hits.iter().enumerate() {
            out.push(Decoration::inline(
                hit.from,
                hit.to,
                Attrs::new().with("class", SEARCH_HIT_CLASS),
            ));
            if current == Some(i) {
                // A second decoration over the same range rather than a
                // two-class string: rinch's view splits overlapping decorations
                // into stretches carrying every class that covers them, so this
                // lands as one span with both — and a hit that is *also*
                // misspelled keeps its squiggle instead of one of the two
                // winning.
                out.push(Decoration::inline(
                    hit.from,
                    hit.to,
                    Attrs::new().with("class", SEARCH_CURRENT_CLASS),
                ));
            }
        }
        DecorationSet::new(out)
    }
}

// ── The app's one search highlight ───────────────────────────────────────────

thread_local! {
    /// One shared state per app: there is one find panel, and it paints into
    /// whichever chapter is open.
    static SHARED: Rc<SearchShared> = SearchShared::new();
}

/// The app-wide find state — the query, the switches, and the current hit.
pub fn shared() -> Rc<SearchShared> {
    SHARED.with(Rc::clone)
}

/// Register the highlighter on `handle`.
///
/// Call this on a freshly created editor, before any content is loaded:
/// `EditorHandle::add_plugin` rebuilds the editor state, which discards the undo
/// history. Registering twice is a no-op.
pub fn register(handle: &crate::rinch_backend::EditorHandle) {
    handle.add_plugin(Rc::new(SearchHighlightPlugin::new(shared())));
}

/// Make an editor re-pull its decorations.
///
/// The view diffs the decoration set it has projected against the one the
/// current state produces, and only does that when a transaction is committed —
/// so a change that alters nothing in the document (a keystroke in the find
/// field, Next moving the current hit, the panel closing) needs an empty
/// transaction to become visible. An empty transaction records no undo step and
/// fires no `on_change`.
pub fn force_redraw(handle: &crate::rinch_backend::EditorHandle) {
    handle.update(|state| Some(state.tr()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rinch_editor_core::model::Fragment;
    use rinch_editor_core::{Node, Pos, Schema, Selection};

    fn doc_of(schema: &Schema, paragraphs: &[&str]) -> Node {
        let blocks: Vec<Node> = paragraphs
            .iter()
            .map(|p| {
                let content = if p.is_empty() {
                    Fragment::empty()
                } else {
                    Fragment::from_node(schema.text(p).unwrap())
                };
                schema.branch("paragraph", content).unwrap()
            })
            .collect();
        schema
            .branch("doc", Fragment::from_children(blocks))
            .unwrap()
    }

    fn plugin() -> (Rc<SearchShared>, Rc<SearchHighlightPlugin>) {
        let shared = SearchShared::new();
        (shared.clone(), Rc::new(SearchHighlightPlugin::new(shared)))
    }

    fn state_with(doc: Node, plugin: Rc<SearchHighlightPlugin>) -> EditorState {
        EditorState::create(Rc::new(Schema::starter_kit()), doc, vec![plugin])
    }

    /// The decorated ranges carrying `class`, in document order.
    fn ranges(set: &DecorationSet, class: &str) -> Vec<(usize, usize)> {
        set.iter()
            .filter(|d| d.inline_class() == Some(class))
            .map(|d| {
                let (from, to) = d.range();
                (from.0, to.0)
            })
            .collect()
    }

    #[test]
    fn a_closed_panel_paints_nothing() {
        let schema = Schema::starter_kit();
        let (_shared, p) = plugin();
        let state = state_with(doc_of(&schema, &["She rode out"]), p);
        assert!(state.decorations().is_empty());
    }

    #[test]
    fn every_hit_is_decorated_over_exactly_its_own_range() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        shared.set_active(true);
        shared.set_search("rode", FindOptions::default());
        let state = state_with(doc_of(&schema, &["She rode out", "and rode back"]), p);
        assert_eq!(
            ranges(&state.decorations(), SEARCH_HIT_CLASS),
            vec![(5, 9), (19, 23)]
        );
        // Nothing is current until something points at one.
        assert!(ranges(&state.decorations(), SEARCH_CURRENT_CLASS).is_empty());
    }

    #[test]
    fn the_current_hit_gets_a_second_decoration_over_the_same_range() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        shared.set_active(true);
        shared.set_search("rode", FindOptions::default());
        shared.set_current(Some(1));
        let state = state_with(doc_of(&schema, &["She rode out", "and rode back"]), p);
        let decos = state.decorations();
        // Both hits still carry the base class...
        assert_eq!(ranges(&decos, SEARCH_HIT_CLASS), vec![(5, 9), (19, 23)]);
        // ...and exactly one of them also carries the current class, over the
        // same range rather than a neighbouring one.
        assert_eq!(ranges(&decos, SEARCH_CURRENT_CLASS), vec![(19, 23)]);
    }

    #[test]
    fn a_current_index_past_the_end_simply_highlights_nothing_extra() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        shared.set_active(true);
        shared.set_search("rode", FindOptions::default());
        shared.set_current(Some(7));
        let state = state_with(doc_of(&schema, &["She rode out"]), p);
        let decos = state.decorations();
        assert_eq!(ranges(&decos, SEARCH_HIT_CLASS), vec![(5, 9)]);
        assert!(ranges(&decos, SEARCH_CURRENT_CLASS).is_empty());
    }

    #[test]
    fn the_switches_change_what_is_painted() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        shared.set_active(true);
        let state = state_with(doc_of(&schema, &["Cat cat catalogue"]), p);

        shared.set_search("cat", FindOptions::default());
        assert_eq!(
            ranges(&state.decorations(), SEARCH_HIT_CLASS),
            vec![(1, 4), (5, 8), (9, 12)]
        );

        shared.set_search(
            "cat",
            FindOptions {
                match_case: true,
                whole_word: false,
            },
        );
        assert_eq!(
            ranges(&state.decorations(), SEARCH_HIT_CLASS),
            vec![(5, 8), (9, 12)]
        );

        shared.set_search(
            "cat",
            FindOptions {
                match_case: false,
                whole_word: true,
            },
        );
        assert_eq!(
            ranges(&state.decorations(), SEARCH_HIT_CLASS),
            vec![(1, 4), (5, 8)]
        );
    }

    #[test]
    fn an_empty_query_paints_nothing_even_while_open() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        shared.set_active(true);
        shared.set_search("", FindOptions::default());
        let state = state_with(doc_of(&schema, &["She rode out"]), p);
        assert!(state.decorations().is_empty());
    }

    #[test]
    fn clearing_the_shared_state_stops_the_painting() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        shared.set_active(true);
        shared.set_search("rode", FindOptions::default());
        let state = state_with(doc_of(&schema, &["She rode out"]), p);
        assert_eq!(state.decorations().iter().count(), 1);
        shared.clear();
        assert!(state.decorations().is_empty());
        assert_eq!(shared.query(), "");
        assert_eq!(shared.current(), None);
    }

    /// The property the panel relies on: a replace is a new state, and the
    /// decorations follow the new document rather than the ranges the search
    /// reported against the old one.
    #[test]
    fn the_highlights_follow_a_replace_without_being_remapped() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        shared.set_active(true);
        shared.set_search("cat", FindOptions::default());
        let state = state_with(doc_of(&schema, &["a cat, a cat"]), p);
        assert_eq!(
            ranges(&state.decorations(), SEARCH_HIT_CLASS),
            vec![(3, 6), (10, 13)]
        );

        // Replace only the first one; the second has shifted by three chars.
        let tr =
            super::super::doc::replace_one_transaction(&state, Pos(3), Pos(6), "kitten").unwrap();
        let next = state.apply(tr);
        assert_eq!(
            ranges(&next.decorations(), SEARCH_HIT_CLASS),
            vec![(13, 16)],
            "the surviving hit is decorated where it now is"
        );

        // And replacing the rest leaves nothing painted at all.
        let tr =
            super::super::doc::replace_all_transaction(&next, "cat", FindOptions::default(), "dog")
                .unwrap();
        let after = next.apply(tr);
        assert!(after.decorations().is_empty());
    }

    #[test]
    fn the_generation_moves_on_every_change_the_panel_makes() {
        let (shared, _p) = plugin();
        let start = shared.generation();
        shared.set_active(true);
        let a = shared.generation();
        assert_ne!(a, start);
        shared.set_search("rode", FindOptions::default());
        let b = shared.generation();
        assert_ne!(b, a);
        shared.set_current(Some(0));
        assert_ne!(shared.generation(), b);
    }

    /// Registered alongside the spellchecker, both contribute: the two plugins
    /// have distinct keys, so adding one does not displace the other.
    #[test]
    fn the_two_decoration_plugins_coexist_in_one_state() {
        let schema = Schema::starter_kit();
        let (shared, search) = plugin();
        shared.set_active(true);
        shared.set_search("rode", FindOptions::default());
        let spell_shared = crate::spell::SpellcheckShared::new(true);
        let spell = Rc::new(crate::spell::SpellcheckPlugin::new(spell_shared));
        let state = EditorState::create(
            Rc::new(Schema::starter_kit()),
            doc_of(&schema, &["She rode out"]),
            vec![search, spell],
        );
        // The spellchecker contributes nothing without a loaded dictionary (a
        // unit test has none), so what is here is the search's, unshadowed.
        assert_eq!(ranges(&state.decorations(), SEARCH_HIT_CLASS), vec![(5, 9)]);
        let _ = Selection::cursor(Pos(0));
    }
}
