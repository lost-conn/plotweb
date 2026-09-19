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

## Settled 2026-09-19 — the span rule for nested events

**A per-book setting, auto-fit by default.** Three rules a book can choose between:

- **Auto-fit** (default): the ribbon draws a parent across its own dates plus all its children's,
  unless the parent is pinned. It is computed at draw time only — typed dates are never rewritten.
- **Clamp**: the parent's typed dates are authoritative; a child running past them is drawn cut
  off at the parent's edge, with a marker.
- **Free**: everything is drawn as typed; a child escaping its parent pokes out and is flagged.

A book that never sets it stores nothing and gets auto-fit.

## (Superseded) Open question — gates step 6 only

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
| Timeline layout — ticks, lanes, tiers, rail (pure, host-tested; card 5) | `plotweb-web/src/pages/book/timeline_layout.rs` |
| Timeline drawing + gestures (card 5) | `plotweb-web/src/pages/book/panes/timeline.rs` |
| Ribbon rules — span rule, cycles, packing, labels (pure, host-tested; card 6) | `plotweb-web/src/pages/book/ribbon.rs` |
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

**Shipped.** The calendar is `plotweb_common::Calendar`: units coarsest first, each `1/per` of
an *earlier* unit (`of`), so units need not nest — a Day is 1/365 Year beside a 1/12 Month. When a
date is written each part is counted inside the part before it by one uniform rule (a unit
belongs to the coarser unit its start falls in), which is what gives the default calendar its
30- and 31-day months without anything knowing what a month is. Stored as whole-value JSON in
`Book::calendar` / `book.json` / the `book:` meta, **absent** for a book that never set one. An
Accord Bell (1/5120 Year) is 6159.375 ticks, so unit starts round down to a whole tick;
`index_at` inverts that exactly, so typed dates read back as typed.

Time entry is one text field in the facet strip covering all five states (`~`, `–`/`to`,
`onward`, `after|before|during <note>`, empty) — see `pages/book/time_entry.rs`. The calendar
screen is an eighth permanently-mounted pane, reached from the notes header and the time field,
not the tools strip. The **holding rail** the wireframe's build order lists under step 4 is
left to card 5, where the build plan puts it; nothing in card 4 draws a timeline to hold it.

Found on the way: the client's `book:` document projected away any facet it had not heard of
(and a note-list refetch *deleted* every span the server did not hold yet, which sync then
carried to the server). Fixed by making a cleared facet a tombstone rather than an absent key
(client and server both), falling back to REST only for a facet the document has never held,
and letting a REST note list fill but never remove or overwrite a facet.

## [Notes 5/8] Timeline — entity lanes

The cheaper half of the timeline, and the half that can't look broken. Ships on its own.

- One horizontal lane per entity; time on the x axis; each event a vertical tie joining the
  lanes of everyone `$` referenced in it, with a dot per participant.
- Lane ordering by first appearance, with an alphabetical toggle.
- A pinned-entity set, or lanes driven by the current filter, so a large cast stays usable.
- Staggered caption tiers with leader lines — dense moments overprint into mush otherwise.
- Undated and relational notes sit in a holding rail beneath, draggable onto the line.

**Done when:** selecting an entity highlights its lane and its participation across the book.

**Shipped.** `NotesView::Timeline` is the second tab. Every rule lives in
`pages/book/timeline_layout.rs` (host-tested); `panes/timeline.rs` only draws it, as SVG built
from the layout, reading the projected note list and never a note body.

- **Cast size: lanes are driven by the shared filter**, not a pinned set, because a pinned set
  would be a second narrowing the tree doesn't know about. The filter works on lanes the way it
  works on the tree: an event is drawn when it matches, and an entity gets a lane when it
  matches, or, **muted**, when a drawn event names it. That's the tree's "ancestor of a match"
  rule.
- **Ticks:** each unit's "shown in timeline when" rule says which units *may* appear for the
  span on screen. The finest one that fits 12 ticks is used, and above that the base unit steps
  by 1/2/5×10ⁿ. A tick reads in full where a coarser part changes ("1818 Jan").
- **An instant spans the unit it is known to.** "1206" is drawn across the year and
  "1206 Mar 4" across the day, so a coarse date doesn't read as a precise one.
- **Entity-less events** sit in a dashed **"no one" row above the lanes**, where the ribbon will
  go, with ordinary captions. A dated entity (a lifespan) is a thick segment on its own lane,
  not a tie.
- **Holding rail:** holds notes with no span that are either relative or `$ref` an entity.
  Plain undated lore stays out. Dropping a note writes a span at the tick unit's precision
  through `note_editor::write_note_time`, the time field's own (tombstone-aware) path. A
  relative constraint the note already has is kept.
- **390px:** the drawing has a 760px minimum width inside its own `overflow-x: auto` box, and
  the page body never scrolls sideways. Lane names scroll away with the drawing (card 7).
- **Found on the way:** rinch's click dispatch reads `className` on every ancestor, and on an
  SVG element that is an `SVGAnimatedString`, so **any `onclick` inside an `<svg>` panics** in
  wasm-bindgen. The drawing is `pointer-events: none`, and transparent HTML boxes laid over it
  take the clicks. The highlight is a reactive class, not part of any item's key, so
  selecting something repaints the drawing without rebuilding it.

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

**Shipped.** Every rule is pure and host-tested in `pages/book/ribbon.rs` (cycle breaking,
the three span rules, escape and clip detection, row packing, label placement);
`timeline_layout.rs` puts the ribbon on card 5's axis and `panes/timeline.rs` draws it.

- **The span rule** is `plotweb_common::SpanRule`, stored beside the calendar: `Book::span_rule`,
  `book.json`, and a plain word at `meta.span_rule` in the `book:` document, all **absent** for
  auto-fit, so choosing auto-fit again stores nothing. The control is a section of the
  **Calendar pane** ("Nested events on the timeline"), and the timeline's toolbar shows the
  current rule as a chip that opens it (an exit, flushed like the time field's link).
- **Pinned** is a facet carried exactly like `is_entity`: `Note::pinned`, `notes.pinned` in the
  `book:` document with `false` as the clear's tombstone, git, mirror, backfill (fingerprint
  only when true), the client projection. The control is a **"Pin dates"** chip in the facet
  strip beside the time field, shown only for a note with a span.
- **Date-less parents are fitted from their children under every rule**, not only auto-fit:
  there are no typed dates to clamp against or escape from, and without an extent the band has
  nowhere to be. Pinning one changes nothing. Such a note leaves the holding rail once it
  contains something dated. **A pinned parent under auto-fit** is drawn as typed and a child
  running past it is flagged as under Free. **Clamp is transitive** (a grandparent's edge cuts
  a grandchild through a date-less middle), and a child wholly outside its parent is drawn as a
  sliver at the edge it ran off.
- **Fitting is computed over every note, then filtered**: a filter that hides a scene does not
  shrink the siege around what is left. A drawn event's non-matching ancestors are drawn muted,
  the tree's rule; they draw no tie and make no lane.
- **Cycles:** `break_cycles` drops the parent link of every note on an `event_parent` loop, so
  each draws as a root and everything after walks a forest (iteratively — a 5000-deep chain is
  a test). A drop that would make a loop is refused before anything is written, with a line
  saying why. An `event_parent` naming an entity, or a deleted note, is not drawn as nesting.
- **Card 5's "no one" row is gone.** With the ribbon on, names are on the bars and card 5's
  caption tiers are not drawn; ties rise from the lanes into each bar and pass *behind* any bar
  they cross. Ribbon off is card 5's view, captions and all, except that an event naming no one
  has nowhere to be and the status line counts it ("1 with no one in them — on the ribbon").
  Both toggles off is not a state: turning off the last zone turns the other back on (and the
  layout reads both-off as both-on). A book with no entities folds the empty lanes zone away.
- **Labels:** whole inside the bar; else whole in the wider gap beside it on its row; else cut
  short inside (≥ 34 units); else cut short beside; else dropped and counted in the status line.
  The wireframe put a truncated name inside whenever there was room for one — preferring the
  whole name beside read better once real titles were in it. Every bar's tooltip is the full
  name plus how the rule drew it ("cut off at the edge of the event it is in").
- **Dragging** is by a `⋮` handle just left of each bar, a sibling of the bar's click target
  (rinch opens on pointerdown, and drops clicks on children of a `draggable`). Bars' click
  targets are the nest drop targets; an overlay on the ribbon zone, armed only during a bar
  drag and beneath the bars, is "take it out". Only `event_parent` is written — the e2e spec
  checks the PUT body and that the tree is byte-identical afterwards.
- **Not from the structure document:** the rule itself is read from `store.current_book`, as
  the calendar is, so a rule changed on another device shows on this one at the next book load
  — the calendar's existing limitation, not a new one.

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
