# Notes revamp — build plan

Design and rationale: `04-notes-wireframes.html`
(published at https://claude.ai/code/artifact/422bed08-1b5a-4cde-a52d-f5f8e93f3dac).

This file is the card text for the build, one section per card. Steps 1–4 carry no risk
from the timeline design — they're model, editor and tree work the ribbon and lanes sit on
top of — so the one open question below only has to be settled before step 6.

## Decisions taken

- **Facets, not fixed types.** One Note. A time span makes it act as an Event; an entity mark
  gives it a thread; neither is lore. A character may carry a lifespan and stay one note.
- **All three sigils kept** — `#tag`, `@mention` (soft link), `$ref` (participation). Only `$`
  draws a thread through an event; `@` stays quiet in the timeline and shows in the editor rail.
- **Notes become a full workspace surface** with a Tree / Timeline switcher, not the 300px pane.
- **Fuzzy + relative time**: exact, approximate, open-ended, relational, undated.
- **Timeline = event ribbon stacked over entity lanes** on one shared axis — not a toggle between
  two views. Ribbon carries nesting and duration; lanes carry presence; each event's tie rises
  from the lanes into its bar. Ribbon off degrades to swimlanes; lanes off degrades to bars
  without the thread-crossing problem.
- **Tree = take E** — one tree the author arranges, facet as a gutter glyph and a filter rather
  than per-facet sections (facets aren't exclusive, so sections stop partitioning).
- **Two separate hierarchies.** `tree_parent` is where the author filed it, arbitrary.
  `event_parent` is containment in time. Different gestures set them, neither implies the other,
  and `event_parent` is **explicit — never inferred from overlapping spans** (the parley and the
  siege coincide without one containing the other).
- **Gregorian deferred.** Divisor stack only; a day is 1/365 of a year and nothing special-cases
  it. Real calendars become their own feature or configuration later, if ever needed.
- **Phone** gets the vertical spine rendering.

## Open question — gates step 6 only

The span rule for nested events. Drawn as *parent auto-fits the union of its children unless
pinned*: add a scene to the siege and the siege grows to contain it. Alternatives are clamping
children to the parent's declared span, or letting a child escape its parent and flagging it.
Auto-fit means you rarely type a date for a parent event at all, which suits drafting, but the
siege's dates then move without you touching them.

## Where the current code lives

| Thing | Where |
|---|---|
| `Note`, `NoteTree`, request types | `crates/plotweb-common/src/lib.rs:362-413` |
| Git note store (still mirrored on write) | `crates/plotweb-git/src/note.rs` |
| Note titles/colors in the book structure doc | `crates/plotweb-crdt/src/book.rs:63-64` |
| Note body docs (`note:{id}`) | `crates/plotweb-crdt/src/body.rs`, `BodyKind::Note` |
| Server handlers | `crates/plotweb-server/src/routes/notes.rs` |
| Cutover helpers (`cutover_structure`, `apply_cutover_structure`) | `crates/plotweb-server/src/routes/mod.rs` |
| Client local-first mirror | `plotweb-web/src/local_book.rs` (`sync_notes`, `note_meta`), `local_store.rs` (`attach_note`) |
| Notes surface — the outline, view switcher, filter bar (card 3) | `plotweb-web/src/pages/book/panes/notes.rs` |
| The filter itself (pure, host-tested) | `plotweb-web/src/pages/book/notes_filter.rs` |
| Note editor pane | `plotweb-web/src/pages/book/panes/note_editor.rs` |
| Note autosave orchestration | `plotweb-web/src/pages/book/mod.rs:1057-1110` |

Verification for every card: `cargo test`, then `cd e2e && npx playwright test` — `cargo test`
does **not** run the e2e suite. Screenshots in both themes at 1280 and 390 where UI changed.

---

## [Notes 1/8] Model — facets, spans, `event_parent`, link index

Extend the note model and mirror what the timeline needs into the book structure doc.

- Add to `Note`: an optional time span (start, end, precision, approximate flag, open-ended
  flag), an optional relational constraint (`after`/`before`/`during` another note id), an
  entity facet flag, and `event_parent: Option<String>` — **distinct from the existing
  `NoteTree` parent, which stays exactly as it is.**
- Derive a link index from note bodies: `#tag`, `@mention`, `$ref` edges per note.
- **Mirror span, facets and link edges up into the `book:` structure doc** alongside the existing
  `note_titles` / `note_colors`. This is the load-bearing part: the timeline must render from
  structure alone, or drawing it means opening every `note:{id}` body doc.
- Migration: every existing note becomes lore — no span, no entity facet, no `event_parent` —
  with its tree position untouched.
- Cutover is now on for every book, so the canonical Automerge doc is authoritative; keep the
  git mirror writing as it does today.

**Done when:** the model round-trips through the server and the client local store, existing
notes are unchanged in the UI, and nothing new is visible yet.

## [Notes 2/8] Sigils and the context rail

Useful on its own, before any new view exists.

- `#`, `@`, `$` autocomplete in the note editor, over existing notes, with "create new" as the
  last entry. `$` offers entities; `@` offers anything; `#` offers existing tags.
- Context rail beside the editor: what this note references, what mentions it, its tags, and —
  once spans exist — its place in time (parent event, child events, simultaneous events).
- Facet strip at the top of the note: event / entity toggles plus the span summary.

Sigil parsing writes into the derived link index from card 1.

**Done when:** typing `$Ve` in a note offers Vess and inserts a real link, and the rail shows
backlinks both ways.

## [Notes 3/8] Tree view (take E) + the workspace surface

- Promote Notes to a top-level workspace view with a **Tree / Timeline** switcher. Filter state
  and selection live above the switcher so changing view is a re-render, not a navigation.
- Replace the horizontal drag-and-drop card tree with a vertical outline: one tree, facet as a
  gutter glyph, facet filter in the header, span shown in a right-aligned gutter.
- Build the shared filter bar here: chips cycling **must / may / mustn't / off**, applying
  identically across every view. (Rename "may" in the UI — it means "at least one of these",
  which "may" doesn't convey.)
- **Retires `render_note_card`**, the 20-parameter recursive function the last design pass
  deliberately left alone (`panes/notes.rs`).

Watch: seven panes are permanently mounted and toggled with `display:none` to preserve editor
undo history (the mount list at the end of `book_page`, `pages/book/mod.rs`; the rule is
documented in `pages/book/panes/mod.rs`). Any routing change must preserve that.

**Done when:** the tree is the notes surface, drag-to-reparent still works, the filter narrows
it, and e2e passes.

**Shipped.** The switcher lives *inside* the still-permanently-mounted Notes pane, so no pane
was unmounted and no routing changed; the Timeline tab renders disabled until card 5 adds a
`NotesView::Timeline`. "may" is called **"any of"** in the UI (off → must → any of → without).

## [Notes 4/8] Calendar and time entry

- Per-book calendar: a base unit plus an ordered list of divisors, each with a label and a
  display rule. Default is Gregorian-shaped — year base, 1/12, 1/365, 1/24 of a day — so a
  contemporary book needs no setup and never sees this screen.
- Timestamps stored as one number in base units plus a precision level; display via a format
  string. **No leap-year or unequal-month handling** — see the deferral above.
- Time entry on a note: exact, approximate, open-ended, relational, or undated.
- Spans surface in the tree gutter. Still no timeline.

**Done when:** a book can define "The Accord" with seasons and bells, and notes can carry spans
in it that sort correctly.

## [Notes 5/8] Timeline — entity lanes

The cheaper half of the timeline, and the half that can't look broken. Ships on its own.

- One horizontal lane per entity; time on the x axis; each event a vertical tie joining the
  lanes of everyone `$` referenced in it, with a dot per participant.
- Lane ordering by first appearance, with an alphabetical toggle.
- A pinned-entity set, or lanes driven by the current filter, so a large cast stays usable.
- Staggered caption tiers with leader lines — dense moments overprint into mush otherwise.
- Undated and relational notes sit in a holding rail beneath, draggable onto the line.

**Done when:** selecting an entity highlights its lane and its participation across the book.

## [Notes 6/8] Timeline — the event ribbon

Where nesting arrives. **Settle the span rule above before starting.**

- Event bars above the lanes, sharing the same x scale, with nested events in sub-rows inside a
  parent containment band.
- Dragging a bar inside another bar sets `event_parent`. Never infer it from overlapping spans.
- Ties from the lanes rise into each event's bar so the two zones read as one drawing.
- `ribbon` / `lanes` toggles collapse either zone; guard against both being off.
- Label placement: inside the bar when it fits, otherwise into whatever gap exists beside it on
  that row, with the full name as a tooltip. Expect some labels to have nowhere to go — the
  wireframe surfaces a live count of these rather than hiding them.

**Done when:** the siege contains the breach visually, and turning the ribbon off leaves a
working lanes view.

## [Notes 7/8] Phone — the vertical spine

- Time runs top to bottom; events are cards on a spine; nesting is indentation; simultaneity is
  an explicit paired row rather than a geometric accident.
- Same data and same filter state as the desktop timeline — a flag, not a second renderer.
- Undated and relational notes sit inline where their constraint puts them, so the holding rail
  isn't needed here.

**Done when:** the notes timeline is usable at 390px wide without horizontal scroll.

## [Notes 8/8] Graph view — backlog

Deferred by the design, as the original card allowed. Notes as nodes, `@mention` and `$ref` as
edges, facet as node shape matching the tree gutter glyphs, sharing the same filter state.
