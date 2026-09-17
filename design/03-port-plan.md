# PlotWeb design port — plan

Agreed direction and mockups: `01-language.html` (tokens, primitives, overlay tiers),
`02-screens.html` (dashboard, book workspace, editor, reader, auth).

**Decided:** Source Sans 3 for UI chrome · quiet-studio voice · measured editor column with
chrome that collapses on typing · content-up/tools-down sidebar · overlays tiered by weight ·
reader gets its own language · refactor depth: "as deep as it takes".

**Out of scope:** the notes surface (being rebuilt later around a timeline — it inherits the
token layer passively and is otherwise untouched). Per-chapter status and per-book word
targets are dropped. Shared-book read-progress is kept.

---

## Stage 0 — rinch repin *(in progress, separate PR)*

`7909da4` → `62cbda6`. Everything below assumes it has landed, because the design depends
on `radius`/`z_index` props being wired (`7a5ffa6`), the Drawer transitioning while closed
(`60d28b9`), and the dismiss/escape/click-outside stack (`b1b47ca`).

---

## Stage 1 — token layer

One place, extending `warm_overrides()` in `plotweb-web/src/components/app_shell.rs`.
Colors stay exactly as they are; everything else is new.

- Type: 6 sizes (`--pw-text-2xs`…`-xl`) replacing 10 ad-hoc literals
- Space: 4px base, 9 steps, replacing 1/2/4/6/8/10/12/16/20/24/32/40/48
- Radius: 3 + pill, replacing 8 treatments
- Elevation: 3 tiers **defined per theme**, plus `--pw-focus-ring` as its own token
  (it is currently smuggled into the shadow list as `0 0 0 2px teal-6`)
- Z-index: 7 named steps — fixes the live bug where the font dropdown (`100`) renders
  beneath the mobile sidebar (`200`)
- Motion: one easing curve, 3 durations, and a `prefers-reduced-motion` block
- Measure: `--pw-measure: 34em`, `--pw-pane-max: 720px`
- Fonts: add Source Sans 3 to the Google Fonts link in `plotweb-web/index.html`; set
  `--pw-font-ui`. Macondo and Playwrite keep the prose and display roles.

**Ships alone and changes nothing visually.** Verify by diffing screenshots before/after.

## Stages 2–6 — each primitive arrives with its first real use

A primitives-only stage would land components nothing imports yet. Instead each one is
extracted at the point a page actually needs it, so every commit is verifiable and no
stage ships dead code.

| Stage | Page | Primitives introduced | Replaces |
|---|---|---|---|
| 2 | **Auth** (4 pages) | `AuthShell` | the 400px card copy-pasted four times |
| 3 | **Dashboard** | `Card`, `BookJacket` | the 180×260 mostly-empty card |
| 4 | **Book workspace** | `PaneHeader`, `SectionHeader`, `Sheet`, `Dialog` | seven hand-built pane headers, five section headers, nine flat modals (~750 lines) |
| 5 | **Editor** | `SelectionToolbar` | the always-on 16-button formatting bar |
| 6 | **Reader** | `Folio`, `FeedbackPopover` | the 320px rail and bordered page bar |

Stage 2 also settles the forgot/reset inconsistency: the same "you're done, go sign in"
moment is a subtle button on one page and a full-width primary on the other.

Stage 4 is the big one — it also splits `book_page` (2,777 lines, ~60 signals) into pane
components, and collapses the ~95%-identical Create/Edit Beta Link modals into one.

Stage 5 touches `pages/editor_utils.rs` as well as `book.rs`; the editor CSS lives there.

## Stage 7 — structural cleanup

`render_note_card` takes 20 parameters and recurses. Notes are out of scope for redesign,
but the signature is worth fixing while the primitives are fresh — **confirm before doing it**,
since the timeline rebuild may delete this code anyway.

---

## Verification

Per stage: `cargo test` · `cd e2e && npx playwright test` · screenshots in both themes at
1280 and 390 wide. Native desktop after Stages 1 and 3 (it has no media queries and its
own text-layout path, so it breaks differently from wasm).

## Watch items

- **Seven panes are permanently mounted** and toggled with `display:none`, deliberately, to
  preserve editor undo history (`book.rs:4398`). Any "route between panes" refactor must
  preserve that or find another way to hold undo state.
- **Breakpoints:** desktop, phone, and native are the targets; tablet is explicitly not.
  Today one 768px breakpoint means 250+300+720 = 1270px holds all the way down to 769px.
  Narrow desktop windows land in that gap even though tablets are not a target.
- **Inline `style:` strings are everywhere** and rinch's pre-stylo inline-style engine was
  removed in the repin span. If Stage 0 surfaces trouble there, prefer moving those to
  classes as part of the page work rather than patching them in place.
