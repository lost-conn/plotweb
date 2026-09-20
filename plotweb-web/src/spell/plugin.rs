//! The editor plugin that turns [`Speller`] verdicts into red squiggles.
//!
//! [`crate::spell`] knows what a word is and whether it is spelled right; this
//! module is the half that draws. It implements rinch's
//! [`Plugin`](rinch_editor_core::Plugin), contributing one
//! [`Decoration::Inline`] with the class `pm-spell-error` per misspelling —
//! which is all rinch's editor view needs to underline it (its default
//! stylesheet already carries `text-decoration: underline wavy`), on the web
//! backend and the native one alike.
//!
//! # Recomputed, not remapped
//!
//! `EditorState::decorations()` calls every plugin afresh for the state being
//! rendered, so there is nothing here that maps ranges through a transaction —
//! and nothing that can drift out of step with the document. The cost of that
//! honesty is that a keystroke re-scans the document, which is why the
//! [`cache`](SpellcheckShared) exists: a block whose text has not changed since
//! the last transaction costs one hash lookup.
//!
//! # Why the cache is keyed on text, not on block identity
//!
//! A block has no stable id in the position space — typing in the first
//! paragraph shifts every position after it, and splitting one renumbers the
//! lot. Its *text* is the only thing that survives an edit elsewhere in the
//! document, so that is the key. Two paragraphs that happen to read the same
//! share one entry, which is a small bonus rather than the point.
//!
//! The entry is invalidated by a **generation** counter rather than by
//! eviction: the author's own words, the book's entity names and the
//! session's ignored words all change what "misspelled" means, and every one of
//! those changes bumps the generation, which empties the cache. See
//! [`SpellcheckShared::bump_generation`].
//!
//! # What the caret suppresses
//!
//! A word being typed is not yet a misspelling. While a collapsed caret sits
//! inside (or against either end of) a token, that token's squiggle is
//! withheld — so the underline appears as the caret leaves the word, the way
//! every other editor behaves. This is applied *after* the cache, so a caret
//! move never invalidates a scan.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rinch_editor_core::model::Fragment;
use rinch_editor_core::{
    Attrs, Decoration, DecorationSet, EditorState, Node, Plugin, PluginKey, Pos, Selection,
};

use super::engine::Speller;
use super::tokenize::tokenize;

/// The stable key for this plugin's slot in an editor state.
pub const SPELLCHECK_KEY: PluginKey = PluginKey("plotweb-spellcheck");

/// The CSS class the view puts on a misspelled run. rinch's built-in editor
/// stylesheet styles this one — `[data-pm-editor] [data-pm-deco].pm-spell-error`
/// — so nothing in plotweb has to describe what a squiggle looks like.
pub const SPELL_ERROR_CLASS: &str = "pm-spell-error";

/// How many block scans to keep. A cache bigger than the document it serves is
/// not a cache; the cap is a ceiling on a pathological document (or a long
/// session of splitting and rejoining blocks), and overflowing it clears rather
/// than evicting, because the next pass refills what is on screen anyway.
const MAX_CACHED_BLOCKS: usize = 2048;

// ── Shared, mutable, outside the plugin ──────────────────────────────────────

/// The switchable, invalidatable part of the spellchecker, shared between the
/// plugin instance inside the editor state and the app code that configures it.
///
/// It lives behind an `Rc` rather than inside `SpellcheckPlugin` because
/// `EditorState` holds its plugins as `Rc<dyn Plugin>` and hands out no way to
/// reach back into one: the toggle, the dictionary changes and the context menu
/// all need a handle on the same state the plugin is reading.
#[derive(Default)]
pub struct SpellcheckShared {
    enabled: Cell<bool>,
    generation: Cell<u64>,
    cache: RefCell<Cache>,
    /// The account's custom words and the open book's entity names, held here as
    /// well as in the speller.
    ///
    /// They have to be: both arrive from asynchronous sources (local storage, the
    /// server, the notes list) that can finish *before* the 860 KB dictionary
    /// does, and a `Speller` that does not exist yet cannot be told anything. The
    /// copy here is what gets installed into the speller the moment it loads (see
    /// [`SpellcheckShared::apply_words`]), which is what makes an author's own
    /// words survive a reload rather than only a session.
    user_words: RefCell<Vec<String>>,
    entity_words: RefCell<Vec<String>>,
}

#[derive(Default)]
struct Cache {
    /// The generation these entries were computed under.
    generation: u64,
    /// Block flat text → the misspelled ranges in it, as char offsets relative
    /// to the block's content start.
    blocks: HashMap<String, Rc<Vec<(usize, usize)>>>,
}

impl SpellcheckShared {
    /// A shared state that starts switched on.
    pub fn new(enabled: bool) -> Rc<SpellcheckShared> {
        let shared = SpellcheckShared::default();
        shared.enabled.set(enabled);
        Rc::new(shared)
    }

    /// Whether squiggles are being drawn.
    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Turn squiggles on or off. The plugin stays registered either way — a
    /// disabled plugin simply contributes nothing, which keeps the undo history
    /// (rebuilding the editor state to drop a plugin would not).
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.set(enabled);
    }

    /// The current dictionary generation.
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Declare that what counts as a misspelling has changed — the account's
    /// custom words, the book's entity names, a session ignore, or the
    /// dictionary itself finishing its load. Empties the cache.
    ///
    /// Note this does **not** repaint: the view pulls decorations when a
    /// transaction is committed, so the caller follows this with
    /// [`crate::spell::plugin::force_redraw`] on the editor handle.
    pub fn bump_generation(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        let mut cache = self.cache.borrow_mut();
        cache.blocks.clear();
        cache.generation = self.generation.get();
    }

    /// Replace the account's custom words and install them.
    pub fn set_user_words(&self, words: Vec<String>) {
        *self.user_words.borrow_mut() = words;
        self.apply_words();
    }

    /// Replace the open book's entity names and install them.
    pub fn set_entity_words(&self, words: Vec<String>) {
        *self.entity_words.borrow_mut() = words;
        self.apply_words();
    }

    /// Push both word lists into the speller, if one has loaded, and invalidate
    /// every cached verdict.
    ///
    /// Called whenever either list changes **and** when the dictionary finishes
    /// loading — the second is the one that matters, because that is the moment a
    /// list that arrived early finally has somewhere to go.
    pub fn apply_words(&self) {
        // Cloned out before the call: `with_loaded_speller` runs arbitrary code
        // that may ask for these lists back.
        let user = self.user_words.borrow().clone();
        let entity = self.entity_words.borrow().clone();
        super::loader::with_loaded_speller(|speller| {
            speller.set_user_words(user);
            speller.set_entity_words(entity.into_iter());
        });
        self.bump_generation();
    }

    /// How many block scans are currently cached (tests, and nothing else).
    #[cfg(test)]
    fn cached_len(&self) -> usize {
        self.cache.borrow().blocks.len()
    }

    /// The misspelled ranges in `text`, from the cache when it has them.
    fn scan_cached(&self, text: &str, speller: &Speller) -> Rc<Vec<(usize, usize)>> {
        let generation = self.generation.get();
        let mut cache = self.cache.borrow_mut();
        if cache.generation != generation {
            cache.blocks.clear();
            cache.generation = generation;
        }
        if let Some(hit) = cache.blocks.get(text) {
            return hit.clone();
        }
        let scanned = Rc::new(scan(text, speller));
        if cache.blocks.len() >= MAX_CACHED_BLOCKS {
            cache.blocks.clear();
        }
        cache.blocks.insert(text.to_string(), scanned.clone());
        scanned
    }
}

// ── The plugin ───────────────────────────────────────────────────────────────

/// Underlines misspellings in a rinch editor.
pub struct SpellcheckPlugin {
    shared: Rc<SpellcheckShared>,
}

impl SpellcheckPlugin {
    /// A plugin reading `shared`.
    pub fn new(shared: Rc<SpellcheckShared>) -> SpellcheckPlugin {
        SpellcheckPlugin { shared }
    }

    /// The shared state this plugin reads.
    pub fn shared(&self) -> &Rc<SpellcheckShared> {
        &self.shared
    }

    /// The decorations for `state`, checked against an explicit `speller`.
    ///
    /// [`Plugin::decorations`] is this with the process-wide speller
    /// ([`crate::spell::loader`]) supplied; the split exists so the tests can
    /// hand in a dictionary of their own.
    pub fn decorations_with(&self, state: &EditorState, speller: &Speller) -> DecorationSet {
        if !self.shared.enabled() {
            return DecorationSet::empty();
        }
        let caret = collapsed_caret(&state.selection);
        let mut out = Vec::new();
        for_each_prose_block(&state.doc, &mut |block, block_pos| {
            let content_start = block_pos + 1;
            let text = block_flat_text(block);
            if text.trim().is_empty() {
                return;
            }
            for (from, to) in self.shared.scan_cached(&text, speller).iter() {
                let (from, to) = (content_start + from, content_start + to);
                // The word under the caret is still being typed.
                if caret.is_some_and(|c| from <= c && c <= to) {
                    continue;
                }
                out.push(Decoration::inline(
                    Pos(from),
                    Pos(to),
                    Attrs::new().with("class", SPELL_ERROR_CLASS),
                ));
            }
        });
        DecorationSet::new(out)
    }
}

impl Plugin for SpellcheckPlugin {
    fn key(&self) -> PluginKey {
        SPELLCHECK_KEY
    }

    fn decorations(&self, state: &EditorState) -> DecorationSet {
        // Nothing at all until the dictionary has loaded — an unchecked document
        // is the honest rendering of "we don't know yet", and the load bumps the
        // generation and forces a repaint when it lands (see `register`).
        let mut set = DecorationSet::empty();
        super::loader::with_loaded_speller(|speller| {
            set = self.decorations_with(state, speller);
        });
        set
    }
}

// ── Finding a word to correct ────────────────────────────────────────────────

/// One misspelling located in the document: its range and the word itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misspelling {
    /// First position of the word.
    pub from: Pos,
    /// Position just past the word.
    pub to: Pos,
    /// The word as the dictionary would look it up (apostrophes folded).
    pub word: String,
}

/// The misspelled word at `pos`, if there is one — what the context menu needs
/// to know before it decides whether to open.
///
/// A position exactly between two words belongs to the one it *ends*, then to
/// the one it starts; a right-click lands on a boundary often enough that
/// silently answering `None` there would read as the menu being broken.
pub fn misspelling_at(doc: &Node, pos: Pos, speller: &Speller) -> Option<Misspelling> {
    let resolved = doc.resolve(pos).ok()?;
    let block = resolved.parent();
    if !block.is_textblock() || block.type_name() == "code_block" {
        return None;
    }
    let content_start = pos.0.checked_sub(resolved.parent_offset())?;
    let text = block_flat_text(block);

    let mut strict: Option<Misspelling> = None;
    let mut touching: Option<Misspelling> = None;
    let mut byte = 0usize;
    let mut ch = 0usize;
    for token in tokenize(&text) {
        ch += text.get(byte..token.range.start)?.chars().count();
        let from = content_start + ch;
        let len = text.get(token.range.clone())?.chars().count();
        ch += len;
        byte = token.range.end;
        let to = from + len;
        if pos.0 < from || pos.0 > to {
            continue;
        }
        if speller.check(&token.text) {
            continue;
        }
        let found = Misspelling {
            from: Pos(from),
            to: Pos(to),
            word: token.text,
        };
        if pos.0 > from && pos.0 < to {
            strict = Some(found);
            break;
        }
        touching.get_or_insert(found);
    }
    strict.or(touching)
}

// ── Document walking ─────────────────────────────────────────────────────────

/// Call `f(block, pos)` for every textblock holding prose, where `pos` is the
/// position **before** the block (so its inline content starts at `pos + 1`).
///
/// Code blocks are skipped whole: an author's `fn`, `impl` and `let` are not
/// misspellings, and underlining them is the fastest way to make someone turn a
/// spellchecker off.
fn for_each_prose_block(doc: &Node, f: &mut impl FnMut(&Node, usize)) {
    doc.nodes_between(0, doc.content_size(), &mut |node, pos, _parent| {
        if !node.is_textblock() {
            // Not a textblock: descend, unless it is a leaf with nothing inside.
            return !node.is_leaf();
        }
        if node.type_name() != "code_block" {
            f(node, pos);
        }
        // Inline content is read through `block_flat_text`, not walked here.
        false
    });
}

/// A textblock's inline content as one flat string whose **chars line up with
/// the position space**: every inline node contributes exactly its own size.
///
/// Non-text inline nodes (an image) become a space, so a word never spans one.
/// So does text carrying the `code` mark — inline code is a identifier, not
/// prose, and masking it rather than dropping it keeps every later offset right.
fn block_flat_text(block: &Node) -> String {
    let mut out = String::new();
    for i in 0..block.child_count() {
        let child = block.child(i);
        match child.text() {
            Some(text) if !is_code(child) => out.push_str(text),
            Some(_) => out.extend(std::iter::repeat_n(' ', child.text_len())),
            None => out.push(' '),
        }
    }
    out
}

/// True if this inline node carries the `code` mark.
fn is_code(node: &Node) -> bool {
    node.marks().iter().any(|m| m.type_name() == "code")
}

/// The position of a collapsed text caret, or `None` for a range/node selection.
fn collapsed_caret(selection: &Selection) -> Option<usize> {
    (selection.is_text_cursor() && selection.is_empty()).then(|| selection.from().0)
}

/// The misspelled ranges in `text`, as `(start, end)` **char** offsets relative
/// to the start of the string.
///
/// The tokenizer works in bytes (it slices the source); the position space
/// counts chars. The two are walked together with one cursor rather than
/// re-counting from the start per token, so a paragraph costs one pass.
fn scan(text: &str, speller: &Speller) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut byte = 0usize;
    let mut ch = 0usize;
    for token in tokenize(text) {
        let Some(gap) = text.get(byte..token.range.start) else {
            break;
        };
        ch += gap.chars().count();
        let start = ch;
        let Some(word) = text.get(token.range.clone()) else {
            break;
        };
        ch += word.chars().count();
        byte = token.range.end;
        if !speller.check(&token.text) {
            out.push((start, ch));
        }
    }
    out
}

// ── Repaint ──────────────────────────────────────────────────────────────────

/// Make an editor re-pull its decorations.
///
/// The view diffs the decoration set it has *projected* against the one the
/// current state produces, and it only does that when a transaction is
/// committed — so a change that alters nothing in the document (the dictionary
/// gaining a word, the toggle flipping, the speller finishing its load) needs an
/// empty transaction to become visible. An empty transaction changes no
/// document, so it records no undo step and fires no `on_change`.
pub fn force_redraw(handle: &crate::rinch_backend::EditorHandle) {
    handle.update(|state| Some(state.tr()));
}

/// Replace the document range `from..to` with `replacement`, keeping the marks
/// the word was wearing.
///
/// The obvious spelling of this — select the word, then paste text over it —
/// loses them: a plain-text replacement carries no marks, so correcting a typo
/// inside an italic line would leave the repair upright. The marks are read off
/// the word's first character (a position strictly inside it, so a boundary
/// between an unmarked and a marked run cannot be read from the wrong side) and
/// put back on the replacement.
///
/// Returns whether the document changed.
pub fn replace_word(
    handle: &crate::rinch_backend::EditorHandle,
    from: Pos,
    to: Pos,
    replacement: &str,
) -> bool {
    if from >= to {
        return false;
    }
    let replacement = replacement.to_string();
    handle.update(move |state| {
        let marks = state
            .doc
            .resolve(Pos(from.0 + 1))
            .ok()
            .map(|r| r.marks())
            .unwrap_or_default();
        let node = state.schema().text_with_marks(&replacement, marks).ok()?;
        let mut tr = state.tr();
        tr.replace_with(from.0, to.0, Fragment::from_node(node))
            .ok()?;
        // Leave the caret just past the repair, where the author would expect it
        // after retyping the word themselves.
        tr.set_selection(Selection::cursor(Pos(from.0 + replacement.chars().count())));
        Some(tr)
    })
}

// ── The app's one spellchecker ───────────────────────────────────────────────

thread_local! {
    /// One shared state per app, because there is one dictionary per app: the
    /// account's words, the open book's names and the switch are the same for the
    /// chapter editor and the note editor, and a second copy would be a second
    /// thing to keep in step.
    static SHARED: Rc<SpellcheckShared> = SpellcheckShared::new(true);
}

/// The app-wide spellcheck state — the switch, the generation and the cache.
pub fn shared() -> Rc<SpellcheckShared> {
    SHARED.with(Rc::clone)
}

/// Register the spellchecker on `handle`, and make sure the dictionary is on its
/// way if the switch is on.
///
/// Call this on a freshly created editor, before any content is loaded:
/// `EditorHandle::add_plugin` rebuilds the editor state, which discards the undo
/// history. Registering twice is a no-op.
pub fn register(handle: &crate::rinch_backend::EditorHandle) {
    if !handle.add_plugin(Rc::new(SpellcheckPlugin::new(shared()))) {
        return;
    }
    if shared().enabled() {
        ensure_dictionary(handle);
    }
}

/// Start (or join) the dictionary load, and repaint `handle` when it lands.
///
/// The ~860 KB en_US pair is fetched once per device and cached, so this is free
/// after the first run — but it is still not asked for until something wants to
/// draw a squiggle, which is why turning the switch on calls it too.
pub fn ensure_dictionary(handle: &crate::rinch_backend::EditorHandle) {
    let handle = handle.clone();
    super::loader::speller(move |result| {
        match result {
            Ok(_) => {
                // A speller built moments ago knows only the bundled word list.
                // Everything the app has learned since startup — the account's
                // words, the book's names — is installed into it here, and "we
                // don't know yet" becomes an answer, so every cached verdict goes.
                shared().apply_words();
                force_redraw(&handle);
            }
            Err(e) => log::warn!("spell: the dictionary did not load: {e}"),
        }
    });
}

/// Flip the switch: update the shared state, repaint, and load the dictionary if
/// this is the first time it has been wanted.
pub fn set_enabled(handle: &crate::rinch_backend::EditorHandle, enabled: bool) {
    shared().set_enabled(enabled);
    if enabled {
        ensure_dictionary(handle);
    }
    force_redraw(handle);
}

/// Install `words` as the open book's entity names and repaint.
pub fn set_entity_words(handle: &crate::rinch_backend::EditorHandle, words: Vec<String>) {
    shared().set_entity_words(words);
    force_redraw(handle);
}

/// Note that the account's custom words changed.
///
/// The list itself is owned by [`crate::local_dictionary`] and has already been
/// handed to [`shared`] by the time this runs (that order is deliberate — see
/// `local_dictionary::install`); this is the half that makes the editor redraw.
pub fn user_words_changed(handle: &crate::rinch_backend::EditorHandle) {
    force_redraw(handle);
}

/// Accept `word` for the rest of this session only, and repaint.
pub fn ignore_for_session(handle: &crate::rinch_backend::EditorHandle, word: &str) {
    let word = word.to_string();
    super::loader::with_loaded_speller(|speller| speller.ignore_session(&word));
    shared().bump_generation();
    force_redraw(handle);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rinch_editor_core::{Mark, Schema};

    const AFF: &str = include_str!("../../../crates/plotweb-server/dictionaries/en_US.aff");
    const DIC: &str = include_str!("../../../crates/plotweb-server/dictionaries/en_US.dic");

    fn speller() -> Speller {
        Speller::from_hunspell(AFF, DIC).expect("the bundled en_US dictionary loads")
    }

    /// A doc of paragraphs, each a single unmarked text node.
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

    /// A state holding `doc` with `plugin` registered, and with the caret rule
    /// deliberately out of the way.
    ///
    /// A freshly created state puts a collapsed caret at the start of the first
    /// block — which is *inside its first word*, so the "still being typed" rule
    /// would withhold that word's squiggle. Every test but the one about that rule
    /// (which sets its own selection afterwards) wants the rule silent, and the way
    /// to silence it honestly is a selection that is not a caret.
    fn state_with(doc: Node, plugin: Rc<SpellcheckPlugin>) -> EditorState {
        let mut state = EditorState::create(Rc::new(Schema::starter_kit()), doc, vec![plugin]);
        if state.doc.content_size() > 2 {
            state.selection = Selection::text(Pos(1), Pos(2));
        }
        state
    }

    /// The decorated ranges, in document order.
    fn ranges(set: &DecorationSet) -> Vec<(usize, usize)> {
        set.iter()
            .filter(|d| d.inline_class() == Some(SPELL_ERROR_CLASS))
            .map(|d| {
                let (from, to) = d.range();
                (from.0, to.0)
            })
            .collect()
    }

    fn plugin() -> (Rc<SpellcheckShared>, Rc<SpellcheckPlugin>) {
        let shared = SpellcheckShared::new(true);
        (shared.clone(), Rc::new(SpellcheckPlugin::new(shared)))
    }

    #[test]
    fn a_misspelling_is_decorated_over_exactly_its_own_range() {
        let schema = Schema::starter_kit();
        let (_shared, p) = plugin();
        // Positions: the paragraph opens at 0, so its text starts at 1.
        //            "She recieved the letter"
        //             1234567890…  `recieved` = chars 4..12 → pos 5..13
        let state = state_with(doc_of(&schema, &["She recieved the letter"]), p.clone());
        let decos = p.decorations_with(&state, &speller());
        assert_eq!(ranges(&decos), vec![(5, 13)]);
        // And it really is the word we think it is.
        let doc = &state.doc;
        let slice = doc.slice(5, 13).unwrap();
        assert_eq!(slice.content.child(0).text(), Some("recieved"));
    }

    #[test]
    fn a_second_block_is_offset_by_the_first_blocks_size() {
        let schema = Schema::starter_kit();
        let (_shared, p) = plugin();
        // "hello" is 5 chars, so the paragraph is 7 wide (0..7) and the second
        // paragraph's text starts at 8.
        let state = state_with(doc_of(&schema, &["hello", "teh"]), p.clone());
        assert_eq!(
            ranges(&p.decorations_with(&state, &speller())),
            vec![(8, 11)]
        );
    }

    #[test]
    fn a_collapsed_caret_in_or_against_the_word_withholds_its_squiggle() {
        let schema = Schema::starter_kit();
        let (_shared, p) = plugin();
        let doc = doc_of(&schema, &["She recieved the letter"]);
        let sp = speller();

        for caret in 5..=13 {
            let mut state = state_with(doc.clone(), p.clone());
            state.selection = Selection::cursor(Pos(caret));
            assert!(
                ranges(&p.decorations_with(&state, &sp)).is_empty(),
                "a caret at {caret} is inside or against `recieved`"
            );
        }
        // One position either side and the squiggle is back.
        for caret in [4usize, 14] {
            let mut state = state_with(doc.clone(), p.clone());
            state.selection = Selection::cursor(Pos(caret));
            assert_eq!(
                ranges(&p.decorations_with(&state, &sp)),
                vec![(5, 13)],
                "a caret at {caret} has left the word"
            );
        }
        // A *selection* over the word is not "being typed", so it still shows.
        let mut state = state_with(doc, p.clone());
        state.selection = Selection::text(Pos(5), Pos(13));
        assert_eq!(ranges(&p.decorations_with(&state, &sp)), vec![(5, 13)]);
    }

    #[test]
    fn an_entity_word_stops_being_a_misspelling() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        let state = state_with(doc_of(&schema, &["Elowen rode out"]), p.clone());

        let mut sp = speller();
        assert_eq!(ranges(&p.decorations_with(&state, &sp)), vec![(1, 7)]);

        sp.set_entity_words(["Elowen".to_string()].into_iter());
        // Without the bump the old answer is still cached — that *is* the cache.
        assert_eq!(ranges(&p.decorations_with(&state, &sp)), vec![(1, 7)]);
        shared.bump_generation();
        assert!(ranges(&p.decorations_with(&state, &sp)).is_empty());
    }

    #[test]
    fn an_unchanged_block_is_scanned_once_and_a_bump_clears_the_cache() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        let state = state_with(doc_of(&schema, &["teh", "teh", "alpha"]), p.clone());
        let sp = speller();

        p.decorations_with(&state, &sp);
        // Two distinct texts ("teh" twice is one entry, "alpha" the other).
        assert_eq!(shared.cached_len(), 2);
        p.decorations_with(&state, &sp);
        assert_eq!(shared.cached_len(), 2, "a re-render adds no entries");

        shared.bump_generation();
        assert_eq!(shared.cached_len(), 0);
    }

    #[test]
    fn marks_splitting_a_word_still_yield_one_decoration() {
        // `recieved` written as three text nodes — `rec` + bold `ie` + `ved` —
        // which is what an author who bolded a couple of letters mid-word leaves
        // behind. The flat text is what gets checked, so it is still one word.
        let schema = Schema::starter_kit();
        let bold = Mark::simple(schema.mark_type("bold").unwrap().clone());
        let para = schema
            .branch(
                "paragraph",
                Fragment::from_children(vec![
                    schema.text("She rec").unwrap(),
                    schema.text_with_marks("ie", vec![bold]).unwrap(),
                    schema.text("ved the letter").unwrap(),
                ]),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(para)).unwrap();

        let (_shared, p) = plugin();
        let state = state_with(doc, p.clone());
        assert_eq!(
            ranges(&p.decorations_with(&state, &speller())),
            vec![(5, 13)]
        );
    }

    #[test]
    fn a_code_block_is_not_prose() {
        let schema = Schema::starter_kit();
        let code = schema
            .branch(
                "code_block",
                Fragment::from_node(schema.text("fn recieve() -> teh {}").unwrap()),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(code)).unwrap();
        let (_shared, p) = plugin();
        let state = state_with(doc, p.clone());
        assert!(ranges(&p.decorations_with(&state, &speller())).is_empty());
    }

    #[test]
    fn inline_code_is_masked_without_shifting_what_follows() {
        let schema = Schema::starter_kit();
        let code = Mark::simple(schema.mark_type("code").unwrap().clone());
        let para = schema
            .branch(
                "paragraph",
                Fragment::from_children(vec![
                    schema.text("Run ").unwrap(),
                    schema.text_with_marks("teh_cmd", vec![code]).unwrap(),
                    schema.text(" then recieve").unwrap(),
                ]),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(para)).unwrap();
        let (_shared, p) = plugin();
        let state = state_with(doc, p.clone());
        // Only the prose word is flagged, and at the offset the masking preserved:
        // "Run " (4) + "teh_cmd" (7) + " then " (6) = 17 chars, +1 for the block.
        assert_eq!(
            ranges(&p.decorations_with(&state, &speller())),
            vec![(18, 25)]
        );
    }

    #[test]
    fn the_switch_silences_the_plugin_without_unregistering_it() {
        let schema = Schema::starter_kit();
        let (shared, p) = plugin();
        let state = state_with(doc_of(&schema, &["teh"]), p.clone());
        assert_eq!(
            ranges(&p.decorations_with(&state, &speller())),
            vec![(1, 4)]
        );
        shared.set_enabled(false);
        assert!(p.decorations_with(&state, &speller()).is_empty());
        shared.set_enabled(true);
        assert_eq!(
            ranges(&p.decorations_with(&state, &speller())),
            vec![(1, 4)]
        );
    }

    #[test]
    fn misspelling_at_finds_the_word_under_a_click_and_ignores_correct_ones() {
        let schema = Schema::starter_kit();
        let doc = doc_of(&schema, &["She recieved the letter"]);
        let sp = speller();

        // Inside the word, and against either edge.
        for pos in [5usize, 9, 13] {
            let found = misspelling_at(&doc, Pos(pos), &sp).expect("a misspelling at {pos}");
            assert_eq!(found.word, "recieved");
            assert_eq!((found.from, found.to), (Pos(5), Pos(13)));
        }
        // `She` and `letter` are words; there is nothing to correct.
        assert_eq!(misspelling_at(&doc, Pos(2), &sp), None);
        assert_eq!(misspelling_at(&doc, Pos(18), &sp), None);
    }

    #[test]
    fn misspelling_at_declines_a_code_block() {
        let schema = Schema::starter_kit();
        let code = schema
            .branch(
                "code_block",
                Fragment::from_node(schema.text("recieve").unwrap()),
            )
            .unwrap();
        let doc = schema.branch("doc", Fragment::from_node(code)).unwrap();
        assert_eq!(misspelling_at(&doc, Pos(4), &speller()), None);
    }

    #[test]
    fn the_plugin_contributes_through_the_state_it_is_registered_in() {
        // The same check as the first test, but through `EditorState::decorations`
        // — the path the view actually uses. Without a loaded dictionary (there is
        // none in a unit test) the honest answer is nothing at all.
        let schema = Schema::starter_kit();
        let (_shared, p) = plugin();
        let state = state_with(doc_of(&schema, &["She recieved the letter"]), p);
        assert!(
            state.decorations().is_empty(),
            "no squiggles before the dictionary has loaded"
        );
    }
}
