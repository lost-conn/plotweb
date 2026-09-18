import { test, expect, Page } from "@playwright/test";
import { openNotesPane, registerNewUser } from "./helpers";

/**
 * The notes workspace surface — the outline (take E), the view switcher and the
 * shared filter bar (notes revamp, card 3).
 *
 * What is guarded here:
 *
 * 1. **The outline renders every note** with its facet glyph and its span gutter, one
 *    tree rather than a section per facet — a character with a lifespan appears once.
 * 2. **The filter narrows it**, including the "any of" group, which is the state the
 *    wireframe flagged as badly named: it means *at least one of these*, so two chips
 *    in it must widen the result rather than intersect it.
 * 3. **An ancestor of a match is kept** (dimmed) instead of dropped, or a match filed
 *    under a non-matching parent would vanish with it.
 * 4. **Opening a note from a row is an exit that saves.** The tree row click changes
 *    `active_pane`, so it has to flush the note editor's pending debounced edit first
 *    (`pages/book/flush.rs`) — this is the notes surface's own exit, alongside the ones
 *    `notes-flush-on-exit.spec.ts` walks.
 * 5. **Touch.** A scrollable list of clickable rows is exactly the shape that broke on
 *    touch before (rinch dispatches `onclick` on pointerdown), and these rows are also
 *    drag sources, so both gestures are driven here: a drag scrolls and opens nothing,
 *    a tap opens.
 *
 * Drag-to-reparent has its own spec (`notes-dnd.spec.ts`) and is deliberately not
 * duplicated.
 */

const NOTE_EDITOR = "#note-editor-main [data-pm-editor]";

/** A row in the outline, by title. */
function row(page: Page, title: string) {
  return page.locator(".note-row", {
    has: page.locator(".note-card-title", { hasText: title }),
  });
}

/** A filter chip, by its label (`entity`, `#siege`, …). */
function chip(page: Page, label: string) {
  return page.locator(".fchip", {
    has: page.locator(".lbl", { hasText: new RegExp(`^${label}$`) }),
  });
}

/** Click a chip `times` times, walking the off → must → any of → without cycle. */
async function cycleChip(page: Page, label: string, times: number) {
  for (let i = 0; i < times; i++) await chip(page, label).click();
}

/** The titles currently rendered in the outline, in order (auto-waiting). */
function titles(page: Page) {
  return page.locator(".notes-tree .note-card-title");
}

/**
 * Make the book over HTTP, *without opening it* — the whole seed has to be on the
 * server before this browser sees the book for the first time.
 *
 * The client mirrors a book's structure (tree, titles, colours, facets, spans) into a
 * local Automerge document, seeded from REST on first open (`local_book::enter`). Card 3
 * found that a facet added by HTTP *after* that first open was projected away, because
 * the local document had never heard of it; card 4 fixed that (`resolve_facets` falls
 * back to REST for a facet the document has never held — see `notes-calendar.spec.ts`).
 * Seeding before the first visit is still the simplest way to put a whole outline on
 * screen in one go.
 */
async function seedBook(page: Page, title: string): Promise<string> {
  const resp = await page.request.post(`/api/books`, {
    data: { title, description: "", cover_image: null },
  });
  if (!resp.ok()) throw new Error(`create book: ${resp.status()}`);
  return ((await resp.json()) as { id: string }).id;
}

/**
 * Create notes over HTTP, with their facets. Same reasoning as [`seedBook`]: call it
 * before the book page is ever opened.
 *
 * Returns the created ids by title.
 */
async function seedNotes(
  page: Page,
  bookId: string,
  notes: Array<{ title: string; parent?: string; patch?: Record<string, unknown> }>,
): Promise<Map<string, string>> {
  const ids = new Map<string, string>();
  for (const n of notes) {
    const resp = await page.request.post(`/api/books/${bookId}/notes`, {
      data: { title: n.title, parent_id: n.parent ? ids.get(n.parent) : null, color: "teal" },
    });
    if (!resp.ok()) throw new Error(`create note ${n.title}: ${resp.status()}`);
    const id = ((await resp.json()) as { id: string }).id;
    ids.set(n.title, id);
    if (n.patch) await patchNote(page, bookId, id, n.patch);
  }
  return ids;
}

/** Give a note a facet / span / tags through the same PATCH the UI uses. */
async function patchNote(
  page: Page,
  bookId: string,
  noteId: string,
  data: Record<string, unknown>,
) {
  const resp = await page.request.put(`/api/books/${bookId}/notes/${noteId}`, { data });
  if (!resp.ok()) throw new Error(`patch note: ${resp.status()}`);
}

/** A body whose text is `text` — the server derives #/@/$ edges from it on write. */
function body(text: string) {
  return JSON.stringify({
    type: "doc",
    content: [{ type: "paragraph", content: [{ type: "text", text }] }],
  });
}

/** One base unit (a year, in the default calendar) in ticks. See `note_time.rs`. */
const YEAR = 31_536_000;

test("the outline shows one tree, with a facet glyph and a span gutter", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await seedBook(page, "Outline Novel");
  // Vess is the case the facet design exists for: an entity that also carries a
  // lifespan. One note, one row — not one per facet.
  await seedNotes(page, bookId, [
    { title: "House Vaun" },
    {
      title: "Vess Calloran",
      patch: {
        is_entity: true,
        span: {
          start: { tick: 1181 * YEAR, precision: 0 },
          end: { tick: 1211 * YEAR, precision: 0 },
          approximate: false,
          open_ended: false,
        },
      },
    },
    {
      title: "The Siege of Vaun",
      patch: {
        span: {
          start: { tick: 1206 * YEAR, precision: 0 },
          approximate: false,
          open_ended: false,
        },
      },
    },
  ]);

  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);

  await expect(titles(page)).toHaveText([
    "House Vaun",
    "Vess Calloran",
    "The Siege of Vaun",
  ]);
  // Entity wins the single gutter glyph; the span still shows beside it.
  await expect(row(page, "Vess Calloran").locator(".note-glyph")).toHaveText("◆");
  await expect(row(page, "Vess Calloran").locator(".note-when")).toHaveText("1181 – 1211");
  await expect(row(page, "The Siege of Vaun").locator(".note-glyph")).toHaveText("◇");
  await expect(row(page, "The Siege of Vaun").locator(".note-when")).toHaveText("1206");
  // Lore keeps the third glyph and an empty gutter.
  await expect(row(page, "House Vaun").locator(".note-glyph")).toHaveText("○");
  await expect(row(page, "House Vaun").locator(".note-when")).toHaveText("");
});

test("the facet filter narrows the tree and keeps the path to a match", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await seedBook(page, "Filter Novel");
  // Vess is filed under House Vaun (lore), which is what makes this the ancestor case.
  await seedNotes(page, bookId, [
    { title: "House Vaun" },
    { title: "Vess Calloran", parent: "House Vaun", patch: { is_entity: true } },
    { title: "Amberwork" },
  ]);

  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await expect(page.locator(".notes-filter-status")).toHaveText("3 notes");

  // One click on the `entity` chip = "must".
  await cycleChip(page, "entity", 1);
  await expect(chip(page, "entity")).toHaveAttribute("data-state", "must");
  await expect(page.locator(".notes-filter-status")).toHaveText("1 of 3 notes · must entity");

  // Amberwork is gone; House Vaun stays as the path to Vess, but dimmed.
  await expect(titles(page)).toHaveText(["House Vaun", "Vess Calloran"]);
  await expect(row(page, "House Vaun")).toHaveClass(/is-muted/);
  await expect(row(page, "Vess Calloran")).not.toHaveClass(/is-muted/);

  // "without" (three more clicks: any of → without) inverts it.
  await cycleChip(page, "entity", 2);
  await expect(chip(page, "entity")).toHaveAttribute("data-state", "without");
  await expect(titles(page)).toHaveText(["House Vaun", "Amberwork"]);

  // Clearing restores the whole tree.
  await page.locator(".notes-filter-clear").click();
  await expect(titles(page)).toHaveText(["House Vaun", "Vess Calloran", "Amberwork"]);
});

test("'any of' means at least one of the group, not all of them", async ({ page }) => {
  // The semantics behind the rename. With two tag chips in the group, a note holding
  // *either* tag must survive — if this behaved like `must`, the result would be empty.
  await registerNewUser(page);
  const bookId = await seedBook(page, "Any Of Novel");
  await seedNotes(page, bookId, [
    { title: "The Siege", patch: { content: body("#siege ") } },
    { title: "Corin's ride", patch: { content: body("#act-two ") } },
    { title: "Amberwork" },
  ]);

  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);

  // Tag chips only exist once something is tagged.
  await expect(chip(page, "#siege")).toBeVisible();
  await expect(chip(page, "#act-two")).toBeVisible();

  // Two clicks each: off → must → any of.
  await cycleChip(page, "#siege", 2);
  await cycleChip(page, "#act-two", 2);
  await expect(chip(page, "#siege")).toHaveAttribute("data-state", "any");
  await expect(titles(page)).toHaveText(["The Siege", "Corin's ride"]);
  await expect(page.locator(".notes-filter-status")).toHaveText(
    "2 of 3 notes · any of #siege, #act-two",
  );

  // The contrast: the same two chips in `must` intersect, and no note holds both
  // tags — so the identical pair of clicks means something else entirely.
  await cycleChip(page, "#siege", 3); // any of → without → off → must
  await cycleChip(page, "#act-two", 3);
  await expect(chip(page, "#siege")).toHaveAttribute("data-state", "must");
  await expect(chip(page, "#act-two")).toHaveAttribute("data-state", "must");
  await expect(page.locator(".notes-empty")).toContainText("No note matches this filter.");
});

test("the view switcher offers Tree now and names the Timeline as not yet here", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await seedBook(page, "Switcher Novel");
  await seedNotes(page, bookId, [{ title: "A note" }]);
  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);

  await expect(page.locator(".notes-viewtab.is-on")).toHaveText("Tree");
  const timeline = page.locator(".notes-viewtab", { hasText: "Timeline" });
  await expect(timeline).toHaveClass(/is-disabled/);
  await expect(timeline).toHaveAttribute("aria-disabled", "true");

  // Clicking it must not swap the outline out for anything.
  await timeline.click();
  await expect(page.locator(".notes-tree")).toBeVisible();
  await expect(page.locator(".notes-viewtab.is-on")).toHaveText("Tree");
});

test("opening a note from a tree row saves the note being left", async ({ page }) => {
  // The surface's own exit. The row click changes `active_pane`, so it must flush the
  // note editor's pending 800ms debounce before the pane moves — see `flush.rs`.
  await registerNewUser(page);
  const bookId = await seedBook(page, "Row Exit Novel");
  await seedNotes(page, bookId, [{ title: "The siege" }, { title: "Vess" }]);
  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await page.locator(".notes-tree .note-card-title", { hasText: "The siege" }).click();
  await page.locator(NOTE_EDITOR).waitFor({ state: "visible", timeout: 15_000 });

  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/notes\//.test(r.url())) {
      writes.push(new URL(r.url()).pathname);
    }
  });

  const text = "Typed, then abandoned by clicking another row.";
  await page.locator(NOTE_EDITOR).click();
  await page.keyboard.type(text);
  expect(writes, "the 800ms debounce must still be pending").toEqual([]);

  // Out through the sidebar to the tree, then straight into the other note's row.
  await openNotesPane(page);
  await page.locator(".notes-tree .note-card-title", { hasText: "Vess" }).click();
  await expect(page.locator(".note-editor-topbar-left")).toContainText("Vess");

  expect(writes, "leaving via a tree row must write the note").not.toEqual([]);

  // And it really is on the server, not just in this tab's model.
  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await page.locator(".notes-tree .note-card-title", { hasText: "The siege" }).click();
  await expect(page.locator(NOTE_EDITOR)).toContainText(text);
});

test("the outline scrolls under a finger and a tap still opens a note", async ({ browser }) => {
  // rinch dispatches `onclick` on pointerdown so drags arm immediately; a scrollable
  // list of clickable rows is the shape that broke on touch. These rows are also drag
  // sources (`draggable="true"`), which takes a different path through rinch's pointer
  // handling than the reader's chapter list — so it is driven here too.
  const ctx = await browser.newContext({
    viewport: { width: 390, height: 500 },
    hasTouch: true,
    isMobile: true,
  });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);

  await registerNewUser(page);
  const bookId = await seedBook(page, "Touch Outline Novel");
  await seedNotes(
    page,
    bookId,
    // Zero-padded so "Row 2" cannot also match "Row 20" when a row is located by title.
    Array.from({ length: 20 }, (_, i) => ({ title: `Row ${String(i + 1).padStart(2, "0")}` })),
  );
  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await expect(page.locator(".note-row").first()).toBeVisible();

  const pane = page.locator(".notes-pane");
  const overflows = await pane.evaluate((el) => el.scrollHeight > el.clientHeight + 4);
  expect(overflows, "the outline must actually overflow for this to mean anything").toBe(true);

  // ── Drag: scrolls the pane, opens nothing. ──
  const box = await row(page, "Row 03").boundingBox();
  if (!box) throw new Error("no row box");
  const x = box.x + box.width / 2;
  const y0 = box.y + box.height / 2;
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y: y0 }] });
  for (let dy = 20; dy <= 200; dy += 20) {
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [{ x, y: y0 - dy }],
    });
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  await expect.poll(() => pane.evaluate((el) => el.scrollTop)).toBeGreaterThan(10);
  await expect(page.locator(NOTE_EDITOR)).toBeHidden();

  // ── Tap: opens the note under the finger. ──
  await pane.evaluate((el) => {
    el.scrollTop = 0;
  });
  const tapBox = await row(page, "Row 02").boundingBox();
  if (!tapBox) throw new Error("no row box (tap)");
  const tx = tapBox.x + tapBox.width / 2;
  const ty = tapBox.y + tapBox.height / 2;
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x: tx, y: ty }],
  });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  await expect(page.locator(".note-editor-topbar-left")).toContainText("Row 02");

  await ctx.close();
});
