import { test, expect, Page, CDPSession } from "@playwright/test";
import { openNotesPane, registerNewUser } from "./helpers";

/**
 * The notes timeline — entity lanes (notes revamp, card 5).
 *
 * What is guarded here:
 *
 * 1. **One lane per entity, one tie per event.** Each event joins the lanes of every
 *    entity it `$ref`s, with a dot per participant; lanes stack by first appearance
 *    and flip to alphabetical on request.
 * 2. **An event naming nobody does not vanish.** It sits in the unassigned row until
 *    card 6's ribbon gives it a home.
 * 3. **Selecting an entity highlights its lane and every event it takes part in**, and
 *    that selection is the tree's selection too — it survives switching view.
 * 4. **The shared filter applies as it does to the tree.** A tag chip narrows the
 *    events, and the lanes those events need stay, muted.
 * 5. **The holding rail.** Relative and undated notes wait beneath the line; dragging
 *    one onto it dates it through the time field's own write path, and the date
 *    survives a reload. Driven by mouse and by a real long-press touch drag.
 * 6. **Opening a note from the timeline is an exit that saves** (`pages/book/flush.rs`).
 * 7. **390px.** The drawing scrolls sideways inside its own box; the page body never
 *    does, and a tap on an event still opens it.
 */

const NOTE_EDITOR = "#note-editor-main [data-pm-editor]";
const YEAR = 31_536_000;

/**
 * Fail on any wasm panic. A panic kills the app without failing whatever assertion
 * happens to come next — an SVG click handler did exactly that during this card (see
 * `Hit` in `panes/timeline.rs`) — so every test here listens for one.
 */
function watchPanics(page: Page): string[] {
  const panics: string[] = [];
  page.on("console", (m) => {
    if (m.text().includes("panicked")) panics.push(m.text().split("\n")[0]);
  });
  return panics;
}

let panics: string[] = [];
test.beforeEach(({ page }) => {
  panics = watchPanics(page);
});
test.afterEach(() => {
  expect(panics, "the app must not panic").toEqual([]);
});

async function seedBook(page: Page, title: string): Promise<string> {
  const resp = await page.request.post(`/api/books`, {
    data: { title, description: "", cover_image: null },
  });
  if (!resp.ok()) throw new Error(`create book: ${resp.status()}`);
  return ((await resp.json()) as { id: string }).id;
}

/** Create notes over HTTP, in order, before the book is first opened. */
async function seedNotes(
  page: Page,
  bookId: string,
  notes: Array<{ title: string; patch?: (ids: Map<string, string>) => Record<string, unknown> }>,
): Promise<Map<string, string>> {
  const ids = new Map<string, string>();
  for (const n of notes) {
    const resp = await page.request.post(`/api/books/${bookId}/notes`, {
      data: { title: n.title, parent_id: null, color: "teal" },
    });
    if (!resp.ok()) throw new Error(`create note ${n.title}: ${resp.status()}`);
    const id = ((await resp.json()) as { id: string }).id;
    ids.set(n.title, id);
    if (n.patch) {
      const put = await page.request.put(`/api/books/${bookId}/notes/${id}`, {
        data: n.patch(ids),
      });
      if (!put.ok()) throw new Error(`patch note ${n.title}: ${put.status()}`);
    }
  }
  return ids;
}

/** A body whose text is `text` — the server derives #/@/$ edges from it on write. */
function body(text: string) {
  return JSON.stringify({
    type: "doc",
    content: [{ type: "paragraph", content: [{ type: "text", text }] }],
  });
}

function year(y: number) {
  return {
    start: { tick: y * YEAR, precision: 0 },
    approximate: false,
    open_ended: false,
  };
}

/**
 * The Amber Chain, cut down: three people, three events that name them, one event
 * that names no one, and two notes waiting in the holding rail.
 */
async function seedCast(page: Page, title: string) {
  const bookId = await seedBook(page, title);
  const ids = await seedNotes(page, bookId, [
    { title: "Vess", patch: () => ({ is_entity: true }) },
    { title: "Corin", patch: () => ({ is_entity: true }) },
    { title: "Maera", patch: () => ({ is_entity: true }) },
    { title: "Harrowgate", patch: () => ({ span: year(1204), content: body("$Vess leaves ") }) },
    {
      title: "Siege",
      patch: () => ({ span: year(1206), content: body("$Corin and $Vess #siege ") }),
    },
    { title: "Winter", patch: () => ({ span: year(1208), content: body("$Maera $Corin ") }) },
    { title: "Comet", patch: () => ({ span: year(1205), content: body("seen by all ") }) },
    {
      title: "Aftermath",
      patch: (ids) => ({ relative: { relation: "after", note_id: ids.get("Siege") } }),
    },
    { title: "Letter", patch: () => ({ content: body("from $Vess ") }) },
  ]);
  return { bookId, ids };
}

async function openTimeline(page: Page) {
  await openNotesPane(page);
  await page.locator(".notes-viewtab", { hasText: "Timeline" }).click();
  await expect(page.locator("#notes-timeline")).toBeVisible();
}

function lanes(page: Page) {
  return page.locator("#notes-timeline .tl-lane");
}

function mark(page: Page, title: string) {
  return page.locator(`#notes-timeline .tl-mark[data-title="${title}"]`);
}

function lane(page: Page, title: string) {
  return page.locator(`#notes-timeline .tl-lane[data-title="${title}"]`);
}

/** A click target laid over the drawing — `mark`, `caption` or `lane`. */
function hit(page: Page, kind: string, title: string) {
  return page.locator(`#notes-timeline .tl-hit-${kind}[data-title="${title}"]`);
}

function held(page: Page, title: string) {
  return page.locator(`#tl-rail .tl-held[data-title="${title}"]`);
}

async function laneOrder(page: Page) {
  return lanes(page).evaluateAll((els) => els.map((e) => e.getAttribute("data-title")));
}

async function notesJson(page: Page, bookId: string): Promise<Array<Record<string, any>>> {
  const r = await page.request.get(`/api/books/${bookId}/notes`);
  expect(r.ok()).toBeTruthy();
  return (await r.json()).notes;
}

test("a lane per entity, a tie per event, and nothing silently dropped", async ({ page }) => {
  await registerNewUser(page);
  const { bookId } = await seedCast(page, "Lanes Novel");
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);

  // By first appearance: Vess at Harrowgate (1204), Corin at the siege (1206), Maera
  // in the winter (1208).
  await expect(lanes(page)).toHaveCount(3);
  expect(await laneOrder(page)).toEqual(["Vess", "Corin", "Maera"]);

  // Each event ties the lanes it $refs, a dot per participant.
  await expect(mark(page, "Siege").locator(".tl-dot")).toHaveCount(2);
  await expect(mark(page, "Harrowgate").locator(".tl-dot")).toHaveCount(1);
  await expect(mark(page, "Winter").locator(".tl-dot")).toHaveCount(2);
  // The tie spans exactly the lanes of the people in it.
  const tie = await mark(page, "Siege").locator(".tl-tie").evaluate((l) => [
    Number(l.getAttribute("y1")),
    Number(l.getAttribute("y2")),
  ]);
  const laneY = async (t: string) =>
    Number(await lane(page, t).locator("line").first().getAttribute("y1"));
  expect(tie).toEqual([await laneY("Vess"), await laneY("Corin")]);

  // The comet names no one: it is drawn in the unassigned row, not dropped.
  await expect(mark(page, "Comet")).toHaveClass(/is-unassigned/);
  await expect(page.locator(".tl-unassigned")).toBeVisible();
  await expect(page.locator(".tl-status")).toContainText("1 with no one in them");

  // Captions carry every event's title, each with a leader line.
  for (const t of ["Harrowgate", "Siege", "Winter", "Comet"]) {
    await expect(page.locator(".tl-caption text", { hasText: t })).toHaveCount(1);
  }
  await expect(page.locator(".tl-caption .tl-leader")).toHaveCount(4);

  // The axis reads through the calendar: years, for a span of years.
  await expect(page.locator(".tl-tick text", { hasText: "1206" })).toHaveCount(1);

  // The holding rail: the relative note and the undated one that names Vess — and
  // not the entities, which are lanes, not waiting events.
  await expect(page.locator("#tl-rail .tl-held")).toHaveCount(2);
  await expect(held(page, "Aftermath")).toContainText("after Siege");
  await expect(held(page, "Letter")).toContainText("undated");

  // Alphabetical on request, and back.
  await page.locator("#tl-order").click();
  await expect(page.locator("#tl-order")).toHaveText("order: alphabetical");
  await expect.poll(() => laneOrder(page)).toEqual(["Corin", "Maera", "Vess"]);
  await page.locator("#tl-order").click();
  await expect.poll(() => laneOrder(page)).toEqual(["Vess", "Corin", "Maera"]);
});

test("selecting an entity highlights its lane and every event it is in", async ({ page }) => {
  await registerNewUser(page);
  const { bookId } = await seedCast(page, "Highlight Novel");
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);

  await page.locator('.tl-chip', { hasText: "Corin" }).click();
  await expect(page.locator('.tl-chip.is-on')).toHaveText("Corin");
  await expect(lane(page, "Corin")).toHaveClass(/is-on/);
  await expect(lane(page, "Vess")).not.toHaveClass(/is-on/);
  await expect(mark(page, "Siege")).toHaveClass(/is-on/);
  await expect(mark(page, "Winter")).toHaveClass(/is-on/);
  await expect(mark(page, "Harrowgate")).not.toHaveClass(/is-on/);
  await expect(mark(page, "Comet")).not.toHaveClass(/is-on/);
  // Corin's dots light, the others on the same ties do not.
  await expect(page.locator(".tl-dot.is-on")).toHaveCount(2);
  await expect(page.locator(".tl-caption.is-on")).toHaveCount(2);

  // It is the tree's selection too: the view switch is a re-render, not a navigation.
  await page.locator(".notes-viewtab", { hasText: "Tree" }).click();
  await expect(
    page.locator(".note-row.is-selected .note-card-title"),
  ).toHaveText("Corin");
  await page.locator(".notes-viewtab", { hasText: "Timeline" }).click();
  await expect(lane(page, "Corin")).toHaveClass(/is-on/);

  // Following nothing: a second click lets go.
  await page.locator('.tl-chip', { hasText: "Corin" }).click();
  await expect(page.locator(".tl-chip.is-on")).toHaveCount(0);
  await expect(page.locator(".tl-mark.is-on")).toHaveCount(0);
});

test("the shared filter narrows the events and keeps the lanes they need, muted", async ({
  page,
}) => {
  await registerNewUser(page);
  const { bookId } = await seedCast(page, "Filter Novel");
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(page.locator("#notes-timeline .tl-mark")).toHaveCount(4);

  // #siege: must.
  await page.locator(".fchip", { has: page.locator(".lbl", { hasText: /^#siege$/ }) }).click();
  await expect(page.locator("#notes-timeline .tl-mark")).toHaveCount(1);
  await expect(mark(page, "Siege")).toBeVisible();
  // Its people keep their lanes — dimmed, as the tree dims an ancestor of a match.
  await expect.poll(() => laneOrder(page)).toEqual(["Corin", "Vess"]);
  await expect(page.locator(".tl-lane.is-muted")).toHaveCount(2);
  // And the rail is filtered too.
  await expect(page.locator("#tl-rail .tl-held")).toHaveCount(0);

  // The same filter, the same answer, in the tree.
  await page.locator(".notes-viewtab", { hasText: "Tree" }).click();
  await expect(page.locator(".notes-tree .note-card-title")).toHaveText(["Siege"]);

  // "entity: must" instead: every lane, no event.
  await page.locator(".notes-filter-clear").click();
  await page.locator(".notes-viewtab", { hasText: "Timeline" }).click();
  await page.locator(".fchip", { has: page.locator(".lbl", { hasText: /^entity$/ }) }).click();
  await expect(page.locator("#notes-timeline .tl-mark")).toHaveCount(0);
  await expect(lanes(page)).toHaveCount(3);
  await expect(page.locator(".tl-lane.is-muted")).toHaveCount(0);
});

/** Drag a held note by mouse to `frac` of the way along the plot. */
async function mouseDropHeld(page: Page, title: string, frac: number) {
  const s = await held(page, title).boundingBox();
  if (!s) throw new Error("held chip has no box");
  const cx = s.x + s.width / 2;
  const cy = s.y + s.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx + 8, cy - 4, { steps: 2 });
  await expect(page.locator("#tl-drop")).toHaveClass(/is-armed/);
  const d = await page.locator("#tl-drop").boundingBox();
  if (!d) throw new Error("drop target has no box");
  const tx = d.x + d.width * frac;
  const ty = d.y + d.height / 2;
  await page.mouse.move(tx, ty, { steps: 10 });
  // The guide says what the drop will write before it is written.
  await expect(page.locator(".tl-drop-label")).toContainText("to the Year");
  await page.mouse.up();
}

test("dragging a held note onto the line dates it, and the date survives a reload", async ({
  page,
}) => {
  await registerNewUser(page);
  const { bookId, ids } = await seedCast(page, "Rail Novel");
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(held(page, "Letter")).toBeVisible();
  // Nothing is written by merely looking.
  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/notes\//.test(r.url())) writes.push(r.url());
  });

  await mouseDropHeld(page, "Letter", 0.5);

  // It left the rail and is on the line, tied to Vess, and selected.
  await expect(held(page, "Letter")).toHaveCount(0);
  await expect(mark(page, "Letter")).toBeVisible();
  await expect(mark(page, "Letter").locator(".tl-dot")).toHaveCount(1);
  await expect(mark(page, "Letter")).toHaveClass(/is-on/);
  await expect(page.locator(".note-drag-ghost")).toBeHidden();
  await expect(page.locator("#tl-drop")).not.toHaveClass(/is-armed/);
  expect(writes.length).toBe(1);

  // On the server: a year-precision instant somewhere inside the drawn span.
  await expect
    .poll(async () => (await notesJson(page, bookId)).find((n) => n.id === ids.get("Letter"))?.span)
    .toBeTruthy();
  const span = (await notesJson(page, bookId)).find((n) => n.id === ids.get("Letter"))!.span;
  expect(span.start.precision).toBe(0);
  expect(span.start.tick % YEAR).toBe(0);
  const y = span.start.tick / YEAR;
  expect(y).toBeGreaterThanOrEqual(1204);
  expect(y).toBeLessThanOrEqual(1208);

  // A relative note dropped keeps its relation: the author said "after the siege".
  await mouseDropHeld(page, "Aftermath", 0.9);
  await expect(mark(page, "Aftermath")).toBeVisible();
  await expect(mark(page, "Aftermath")).toHaveClass(/is-unassigned/);
  await expect
    .poll(async () => (await notesJson(page, bookId)).find((n) => n.id === ids.get("Aftermath"))?.span)
    .toBeTruthy();
  const after = (await notesJson(page, bookId)).find((n) => n.id === ids.get("Aftermath"))!;
  expect(after.relative).toEqual({ relation: "after", note_id: ids.get("Siege") });

  // Survives a reload — the local document and the server agree.
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(mark(page, "Letter")).toBeVisible();
  await expect(mark(page, "Aftermath")).toBeVisible();
  await expect(page.locator("#tl-rail .tl-held")).toHaveCount(0);
  // And the tree gutter reads the same date.
  await page.locator(".notes-viewtab", { hasText: "Tree" }).click();
  await expect(
    page.locator(".note-row", { has: page.locator(".note-card-title", { hasText: "Letter" }) }).locator(".note-when"),
  ).toHaveText(String(y));
});

test("opening an event from the timeline saves the note being left", async ({ page }) => {
  // The timeline's own exit, alongside the tree row's. Clicking an event changes
  // `active_pane`, so it goes through `open_note_row`, which flushes first.
  await registerNewUser(page);
  const { bookId } = await seedCast(page, "Timeline Exit Novel");
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);

  // Open the siege from its band.
  await hit(page, "mark", "Siege").click();
  await page.locator(NOTE_EDITOR).waitFor({ state: "visible", timeout: 15_000 });
  await expect(page.locator(".note-editor-topbar-left")).toContainText("Siege");

  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/notes\//.test(r.url())) {
      writes.push(new URL(r.url()).pathname);
    }
  });
  const text = "Typed, then abandoned for the timeline.";
  const surface = page.locator(NOTE_EDITOR);
  await surface.click();
  await page.keyboard.press("End");
  await page.keyboard.type(text, { delay: 10 });
  await expect(surface).toContainText(text);
  expect(writes, "the 800ms debounce must still be pending").toEqual([]);

  // Back to the notes surface — which remembers it was on the timeline — then
  // straight into a lane's name.
  await openNotesPane(page);
  await expect(page.locator("#notes-timeline")).toBeVisible();
  await hit(page, "lane", "Vess").click();
  await expect(page.locator(".note-editor-topbar-left")).toContainText("Vess");
  expect(writes, "leaving must write the note").not.toEqual([]);

  // And it is on the server.
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await hit(page, "caption", "Siege").click();
  await expect(page.locator(NOTE_EDITOR)).toContainText(text);
});

test.describe("phone", () => {
  async function touch(cdp: CDPSession, type: string, x?: number, y?: number) {
    await cdp.send("Input.dispatchTouchEvent", {
      type,
      touchPoints: x === undefined ? [] : [{ x, y }],
    });
  }

  test("at 390px the drawing scrolls in its own box, and touch opens and drops", async ({
    browser,
  }) => {
    const ctx = await browser.newContext({
      viewport: { width: 390, height: 780 },
      hasTouch: true,
      isMobile: true,
    });
    const page = await ctx.newPage();
    panics = watchPanics(page);
    const cdp = await ctx.newCDPSession(page);
    await registerNewUser(page);
    const { bookId, ids } = await seedCast(page, "Phone Timeline Novel");
    await page.goto(`/book/${bookId}`);
    await openTimeline(page);
    await expect(mark(page, "Siege")).toBeAttached();

    // The page body never scrolls sideways; the drawing's box does.
    const body = await page.evaluate(() => ({
      scroll: document.documentElement.scrollWidth,
      client: document.documentElement.clientWidth,
    }));
    expect(body.scroll).toBeLessThanOrEqual(body.client);
    const box = page.locator(".tl-scroll");
    expect(await box.evaluate((el) => el.scrollWidth > el.clientWidth + 10)).toBe(true);

    // A long-press drag from the rail onto the visible part of the line dates it.
    const chip = await held(page, "Letter").boundingBox();
    const drop = await page.locator(".tl-scroll").boundingBox();
    if (!chip || !drop) throw new Error("no boxes");
    const sx = chip.x + chip.width / 2;
    const sy = chip.y + chip.height / 2;
    await touch(cdp, "touchStart", sx, sy);
    await page.waitForTimeout(450);
    const tx = drop.x + drop.width * 0.75;
    const ty = drop.y + drop.height / 2;
    for (let i = 1; i <= 8; i++) {
      await touch(cdp, "touchMove", sx + ((tx - sx) * i) / 8, sy + ((ty - sy) * i) / 8);
    }
    await touch(cdp, "touchEnd");
    await expect(mark(page, "Letter")).toBeAttached();
    await expect(held(page, "Letter")).toHaveCount(0);
    await expect
      .poll(async () => (await notesJson(page, bookId)).find((n) => n.id === ids.get("Letter"))?.span)
      .toBeTruthy();

    // A tap on a held chip opens it — a tap is not a drag.
    const a = await held(page, "Aftermath").boundingBox();
    if (!a) throw new Error("no chip");
    await touch(cdp, "touchStart", a.x + a.width / 2, a.y + a.height / 2);
    await touch(cdp, "touchEnd");
    await expect(page.locator(".note-editor-topbar-left")).toContainText("Aftermath");

    // Back (the editor's own arrow — the sidebar is a drawer at this width), and a tap
    // on the siege's caption opens it.
    await page.locator(".note-editor-topbar .rinch-action-icon").first().click();
    await expect(page.locator("#notes-timeline")).toBeVisible();
    // The dropped note's drag ended cleanly: no ghost left floating, no armed target.
    await expect(page.locator(".note-drag-ghost")).toBeHidden();
    await expect(page.locator("#tl-drop")).not.toHaveClass(/is-armed/);
    // The siege is off to the right in the drawing's own scroller — bring it in.
    await hit(page, "caption", "Siege").scrollIntoViewIfNeeded();
    const cap = await hit(page, "caption", "Siege").boundingBox();
    if (!cap) throw new Error("no caption");
    await touch(cdp, "touchStart", cap.x + cap.width / 2, cap.y + cap.height / 2);
    await touch(cdp, "touchEnd");
    await expect(page.locator(".note-editor-topbar-left")).toContainText("Siege");

    await ctx.close();
  });
});
