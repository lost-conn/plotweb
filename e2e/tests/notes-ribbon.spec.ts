import { test, expect, Page, CDPSession } from "@playwright/test";
import { openNotesPane, registerNewUser } from "./helpers";

/**
 * The notes timeline — the event ribbon (notes revamp, card 6).
 *
 * What is guarded here:
 *
 * 1. **Nesting is a drag, and only a drag.** Dragging a bar's handle onto another bar
 *    sets its `event_parent`; dragging it onto empty ribbon clears it. Both survive a
 *    reload, and neither moves the note in the tree — `event_parent` and the tree parent
 *    are separate hierarchies. Overlapping spans never nest by themselves.
 * 2. **A drop that would make a loop is refused**, and writes nothing.
 * 3. **The book's span rule.** Under auto-fit the siege stretches over the breach;
 *    under clamp the breach is cut off at the siege's edge, marked; under free it pokes
 *    out, flagged. Switching the rule (Calendar pane) redraws without a reload and is
 *    stored on the book; auto-fit stores nothing.
 * 4. **Pinning** keeps a parent's own dates under auto-fit, and survives a reload.
 * 5. **Ribbon / lanes toggles** collapse either zone, and never both.
 * 6. **Labels that have nowhere to go are counted aloud.**
 * 7. **Touch, at a width that still shows the ribbon:** a long-press on a handle drags
 *    a bar into another. Card 7 (notes revamp) gave the *phone* width (≤768px) a
 *    different drawing — the vertical spine, `notes-timeline-spine.spec.ts` — so this
 *    drag-by-handle touch coverage now runs at a wider touch viewport, where `.tl` is
 *    still the one on screen.
 */

const YEAR = 31_536_000;

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

function body(text: string) {
  return JSON.stringify({
    type: "doc",
    content: [{ type: "paragraph", content: [{ type: "text", text }] }],
  });
}

function years(from: number, to?: number) {
  return {
    start: { tick: from * YEAR, precision: 0 },
    end: to === undefined ? null : { tick: to * YEAR, precision: 0 },
    approximate: false,
    open_ended: false,
  };
}

/**
 * The siege (1206–1207) and the breach (1207–1209), which runs a year past it; the
 * parley (1206) coincides with the siege without being in it. `nested` puts the breach
 * inside the siege from the start.
 */
async function seedSiege(page: Page, title: string, nested = false) {
  const bookId = await seedBook(page, title);
  const ids = await seedNotes(page, bookId, [
    { title: "Vess", patch: () => ({ is_entity: true }) },
    { title: "Corin", patch: () => ({ is_entity: true }) },
    { title: "Siege", patch: () => ({ span: years(1206, 1207), content: body("$Corin and $Vess ") }) },
    {
      title: "Breach",
      patch: (ids) => ({
        span: years(1207, 1209),
        content: body("$Corin ") ,
        ...(nested ? { event_parent: ids.get("Siege") } : {}),
      }),
    },
    { title: "Parley", patch: () => ({ span: years(1206), content: body("$Vess ") }) },
  ]);
  return { bookId, ids };
}

async function openTimeline(page: Page) {
  await openNotesPane(page);
  await page.locator(".notes-viewtab", { hasText: "Timeline" }).click();
  await expect(page.locator("#notes-timeline")).toBeVisible();
}

function bar(page: Page, title: string) {
  return page.locator(`#notes-timeline .tl-event[data-title="${title}"]`);
}

function barHit(page: Page, title: string) {
  return page.locator(`#notes-timeline .tl-hit-bar[data-title="${title}"]`);
}

function handle(page: Page, title: string) {
  return page.locator(`#notes-timeline .tl-handle[data-title="${title}"]`);
}

async function notesJson(page: Page, bookId: string): Promise<{ notes: any[]; tree: any }> {
  const r = await page.request.get(`/api/books/${bookId}/notes`);
  expect(r.ok()).toBeTruthy();
  return await r.json();
}

async function eventParent(page: Page, bookId: string, id: string) {
  return (await notesJson(page, bookId)).notes.find((n) => n.id === id)?.event_parent ?? null;
}

/** Where a bar is drawn, in the SVG's own units. */
async function barX(page: Page, title: string) {
  return bar(page, title)
    .locator(".tl-event-bar")
    .evaluate((r) => {
      const x = Number(r.getAttribute("x"));
      return { x0: x, x1: x + Number(r.getAttribute("width")) };
    });
}

/** Drag a bar by its handle, by mouse, onto `target` (a bar title) or the empty ribbon. */
async function mouseDragBar(page: Page, title: string, target: string | null) {
  const h = await handle(page, title).boundingBox();
  if (!h) throw new Error(`no handle for ${title}`);
  const sx = h.x + h.width / 2;
  const sy = h.y + h.height / 2;
  await page.mouse.move(sx, sy);
  await page.mouse.down();
  await page.mouse.move(sx + 6, sy + 4, { steps: 2 });
  await expect(page.locator("#tl-ribbon-drop")).toHaveClass(/is-armed/);
  let tx: number;
  let ty: number;
  if (target) {
    const t = await barHit(page, target).boundingBox();
    if (!t) throw new Error(`no bar for ${target}`);
    tx = t.x + t.width / 2;
    ty = t.y + t.height / 2;
  } else {
    // Empty ribbon: the drop zone's bottom-right corner, clear of every bar.
    const z = await page.locator("#tl-ribbon-drop").boundingBox();
    if (!z) throw new Error("no ribbon drop zone");
    tx = z.x + z.width - 12;
    ty = z.y + z.height - 4;
  }
  await page.mouse.move(tx, ty, { steps: 10 });
  await page.mouse.up();
}

test("dragging a bar into another nests it, dragging it out un-nests it, and the tree never moves", async ({
  page,
}) => {
  await registerNewUser(page);
  const { bookId, ids } = await seedSiege(page, "Nesting Novel");
  const treeBefore = (await notesJson(page, bookId)).tree;
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);

  // Overlap is not nesting: the parley coincides with the siege, and nothing is nested.
  await expect(bar(page, "Breach")).toHaveClass(/depth-0/);
  await expect(bar(page, "Parley")).toHaveClass(/depth-0/);
  await expect(page.locator(".tl-contain")).toHaveCount(0);

  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/notes\//.test(r.url())) writes.push(r.postData() ?? "");
  });

  await mouseDragBar(page, "Breach", "Siege");
  await expect(bar(page, "Breach")).toHaveClass(/depth-1/);
  await expect(page.locator(`.tl-contain[data-id="${ids.get("Siege")}"]`)).toHaveCount(1);
  // Auto-fit, the default: the siege now spans the breach.
  await expect(bar(page, "Siege")).toHaveClass(/is-fitted/);
  const [s, b] = [await barX(page, "Siege"), await barX(page, "Breach")];
  expect(s.x1).toBeGreaterThanOrEqual(b.x1 - 0.1);
  await expect(page.locator(".note-drag-ghost")).toBeHidden();
  await expect(page.locator("#tl-ribbon-drop")).not.toHaveClass(/is-armed/);
  // Exactly one write, and it names only the event parent.
  expect(writes.length).toBe(1);
  const sent = JSON.parse(writes[0]);
  expect(sent.event_parent).toBe(ids.get("Siege"));
  for (const k of ["span", "relative", "is_entity", "pinned"]) expect(k in sent).toBe(false);
  for (const k of ["title", "content", "color"]) expect(sent[k] ?? null).toBe(null);
  await expect.poll(() => eventParent(page, bookId, ids.get("Breach")!)).toBe(ids.get("Siege"));

  // Survives a reload.
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(bar(page, "Breach")).toHaveClass(/depth-1/);

  // Out onto empty ribbon: a clear.
  await mouseDragBar(page, "Breach", null);
  await expect(bar(page, "Breach")).toHaveClass(/depth-0/);
  await expect(page.locator(".tl-contain")).toHaveCount(0);
  await expect(bar(page, "Siege")).not.toHaveClass(/is-fitted/);
  await expect.poll(() => eventParent(page, bookId, ids.get("Breach")!)).toBe(null);
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(bar(page, "Breach")).toHaveClass(/depth-0/);

  // Neither gesture touched the tree: every note is where the author filed it.
  expect((await notesJson(page, bookId)).tree).toEqual(treeBefore);
  await page.locator(".notes-viewtab", { hasText: "Tree" }).click();
  await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
    "Vess",
    "Corin",
    "Siege",
    "Breach",
    "Parley",
  ]);
});

test("a drop that would make a loop is refused and writes nothing", async ({ page }) => {
  await registerNewUser(page);
  const { bookId, ids } = await seedSiege(page, "Loop Novel", true);
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(bar(page, "Breach")).toHaveClass(/depth-1/);

  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/notes\//.test(r.url())) writes.push(r.url());
  });
  // The siege into the breach, which is inside it.
  await mouseDragBar(page, "Siege", "Breach");
  await expect(page.locator("#tl-refusal")).toContainText("cannot hold what holds it");
  await expect(bar(page, "Siege")).toHaveClass(/depth-0/);
  expect(writes).toEqual([]);
  expect(await eventParent(page, bookId, ids.get("Siege")!)).toBe(null);
});

async function setRule(page: Page, rule: "fit" | "clamp" | "free") {
  await page.locator("#tl-rule").click();
  await expect(page.locator("#calendar-pane")).toBeVisible();
  await page.locator(`#span-rule-${rule}`).click();
  await expect(page.locator(`#span-rule-${rule}`)).toHaveClass(/is-on/);
  await expect(page.locator("#span-rule-status")).toHaveText("Saved");
  await page.locator(".calendar-head .rinch-action-icon").click();
  await expect(page.locator("#notes-timeline")).toBeVisible();
}

async function bookJson(page: Page, bookId: string) {
  const r = await page.request.get(`/api/books/${bookId}`);
  expect(r.ok()).toBeTruthy();
  return await r.json();
}

test("each span rule contains, clips or flags, and switching redraws without a reload", async ({
  page,
}) => {
  await registerNewUser(page);
  const { bookId } = await seedSiege(page, "Rules Novel", true);
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  // A book that never chose stores nothing, and reads as auto-fit.
  expect((await bookJson(page, bookId)).span_rule).toBeUndefined();
  await expect(page.locator("#tl-rule")).toHaveText("nesting: auto-fit");

  // Auto-fit: contained.
  await expect(bar(page, "Siege")).toHaveClass(/is-fitted/);
  await expect(bar(page, "Breach")).not.toHaveClass(/is-clipped|is-escaping/);
  let [s, b] = [await barX(page, "Siege"), await barX(page, "Breach")];
  expect(s.x1).toBeGreaterThanOrEqual(b.x1 - 0.1);

  // Clamp: the breach is cut at the siege's edge, and marked.
  await setRule(page, "clamp");
  await expect(page.locator("#tl-rule")).toHaveText("nesting: clamp");
  await expect(bar(page, "Siege")).not.toHaveClass(/is-fitted/);
  await expect(bar(page, "Breach")).toHaveClass(/is-clipped/);
  await expect(bar(page, "Breach").locator(".tl-clip-end")).toHaveCount(1);
  [s, b] = [await barX(page, "Siege"), await barX(page, "Breach")];
  expect(Math.abs(s.x1 - b.x1)).toBeLessThan(0.2);
  await expect(barHit(page, "Breach")).toHaveAttribute("title", /cut off/);

  // Free: the breach pokes out past the band, flagged.
  await setRule(page, "free");
  await expect(bar(page, "Breach")).toHaveClass(/is-escaping/);
  await expect(bar(page, "Breach").locator(".tl-escape-end")).toHaveCount(1);
  [s, b] = [await barX(page, "Siege"), await barX(page, "Breach")];
  expect(b.x1).toBeGreaterThan(s.x1 + 10);
  const band = await page.locator(".tl-contain").evaluate((r) =>
    Number(r.getAttribute("x")) + Number(r.getAttribute("width")),
  );
  expect(b.x1).toBeGreaterThan(band);

  // Stored on the book, and still there after a reload.
  expect((await bookJson(page, bookId)).span_rule).toBe("free");
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(bar(page, "Breach")).toHaveClass(/is-escaping/);

  // Back to auto-fit stores nothing at all again.
  await setRule(page, "fit");
  await expect(bar(page, "Siege")).toHaveClass(/is-fitted/);
  expect((await bookJson(page, bookId)).span_rule).toBeUndefined();

  // No rule ever rewrote a typed date.
  const notes = (await notesJson(page, bookId)).notes;
  const siege = notes.find((n) => n.title === "Siege");
  expect(siege.span.end.tick).toBe(1207 * YEAR);
});

test("pinning a parent keeps its own dates under auto-fit, and survives a reload", async ({
  page,
}) => {
  await registerNewUser(page);
  const { bookId, ids } = await seedSiege(page, "Pin Novel", true);
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(bar(page, "Siege")).toHaveClass(/is-fitted/);

  // The pin lives in the note's facet strip, beside its time.
  await barHit(page, "Siege").click();
  await expect(page.locator(".note-editor-topbar-left")).toContainText("Siege");
  await expect(page.locator("#note-facet-pin")).toHaveText(/Pin dates/);
  await page.locator("#note-facet-pin").click();
  await expect(page.locator("#note-facet-pin")).toHaveClass(/is-on/);
  await expect
    .poll(async () => (await notesJson(page, bookId)).notes.find((n) => n.id === ids.get("Siege"))?.pinned)
    .toBe(true);

  await openTimeline(page);
  await expect(bar(page, "Siege")).toHaveClass(/is-pinned/);
  await expect(bar(page, "Siege")).not.toHaveClass(/is-fitted/);
  await expect(bar(page, "Breach")).toHaveClass(/is-escaping/);

  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(bar(page, "Siege")).toHaveClass(/is-pinned/);

  // Unpinned: fitted again, and the stored value is a clear, not a leftover.
  await barHit(page, "Siege").click();
  await page.locator("#note-facet-pin").click();
  await expect(page.locator("#note-facet-pin")).not.toHaveClass(/is-on/);
  await expect
    .poll(async () => (await notesJson(page, bookId)).notes.find((n) => n.id === ids.get("Siege"))?.pinned ?? false)
    .toBe(false);
  await openTimeline(page);
  await expect(bar(page, "Siege")).toHaveClass(/is-fitted/);
});

test("the ribbon and lanes toggles collapse either zone, never both", async ({ page }) => {
  await registerNewUser(page);
  const { bookId } = await seedSiege(page, "Zones Novel", true);
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(page.locator("#notes-timeline .tl-event")).toHaveCount(3);
  await expect(page.locator("#notes-timeline .tl-lane")).toHaveCount(2);
  // The ties rise from the lanes into the bars: the siege's tie starts at its bar.
  const tieTop = await page
    .locator(`#notes-timeline .tl-mark[data-title="Siege"] .tl-tie`)
    .evaluate((l) => Number(l.getAttribute("y1")));
  const barBottom = await bar(page, "Siege")
    .locator(".tl-event-bar")
    .evaluate((r) => Number(r.getAttribute("y")) + Number(r.getAttribute("height")));
  expect(Math.abs(tieTop - barBottom)).toBeLessThan(0.2);
  await expect(page.locator(".tl-caption")).toHaveCount(0);

  // Ribbon off: card 5's lanes, with captions.
  await page.locator("#tl-ribbon").click();
  await expect(page.locator("#tl-ribbon")).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#notes-timeline .tl-event")).toHaveCount(0);
  await expect(page.locator(".tl-caption")).toHaveCount(3);

  // Lanes off too would leave nothing: the ribbon comes back instead.
  await page.locator("#tl-lanes").click();
  await expect(page.locator("#tl-lanes")).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#tl-ribbon")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#notes-timeline .tl-event")).toHaveCount(3);
  await expect(page.locator("#notes-timeline .tl-lane")).toHaveCount(0);
  await expect(page.locator("#notes-timeline .tl-mark")).toHaveCount(0);

  // Selection still lights the ribbon with the lanes off (via the follow chips).
  await page.locator(".tl-chip", { hasText: "Corin" }).click();
  await expect(bar(page, "Siege")).toHaveClass(/is-on/);
  await expect(bar(page, "Breach")).toHaveClass(/is-on/);
  await expect(bar(page, "Parley")).not.toHaveClass(/is-on/);

  await page.locator("#tl-ribbon").click();
  await expect(page.locator("#tl-lanes")).toHaveAttribute("aria-pressed", "true");
});

test("labels with nowhere to go are counted, and keep their name as a tooltip", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await seedBook(page, "Crowd Novel");
  const notes: Array<{ title: string; patch?: (ids: Map<string, string>) => Record<string, unknown> }> = [
    { title: "The long war", patch: () => ({ span: years(1200, 1230) }) },
  ];
  // Twelve month-long scenes, six months apart: they share one row (none overlaps the
  // next), each far too narrow to hold its name, with too little gap between them.
  for (let d = 1; d <= 12; d++) {
    notes.push({
      title: `A rather long scene name ${d}`,
      patch: () => ({
        span: {
          start: { tick: 1210 * YEAR + (d * YEAR) / 2, precision: 1 },
          end: null,
          approximate: false,
          open_ended: false,
        },
      }),
    });
  }
  await seedNotes(page, bookId, notes);
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(page.locator(".tl-status")).toContainText(/labels? with nowhere to go/);
  const title = await barHit(page, "A rather long scene name 7").getAttribute("title");
  expect(title).toContain("A rather long scene name 7");
});

test.describe("touch", () => {
  async function touch(cdp: CDPSession, type: string, x?: number, y?: number) {
    await cdp.send("Input.dispatchTouchEvent", {
      type,
      touchPoints: x === undefined ? [] : [{ x, y }],
    });
  }

  // 800px, not 390px: card 7 (notes revamp) swaps the Timeline tab to the vertical
  // spine at ≤768px, which has no drag-to-nest handle (see `spine_layout.rs` and
  // `notes-timeline-spine.spec.ts`). This spec is about the ribbon's own touch-drag
  // nesting, so it now runs at a width that still shows `.tl`, hasTouch/isMobile kept
  // so the gesture itself is still exercised on a touch viewport.
  test("a long-press on a handle drags a bar into another, and a tap opens one", async ({
    browser,
  }) => {
    const ctx = await browser.newContext({
      viewport: { width: 800, height: 780 },
      hasTouch: true,
      isMobile: true,
    });
    const page = await ctx.newPage();
    panics = watchPanics(page);
    const cdp = await ctx.newCDPSession(page);
    await registerNewUser(page);
    const { bookId, ids } = await seedSiege(page, "Phone Ribbon Novel");
    await page.goto(`/book/${bookId}`);
    await openTimeline(page);
    await expect(bar(page, "Breach")).toHaveClass(/depth-0/);

    // The page body never scrolls sideways.
    const body = await page.evaluate(() => ({
      scroll: document.documentElement.scrollWidth,
      client: document.documentElement.clientWidth,
    }));
    expect(body.scroll).toBeLessThanOrEqual(body.client);

    await handle(page, "Breach").scrollIntoViewIfNeeded();
    const h = await handle(page, "Breach").boundingBox();
    const t = await barHit(page, "Siege").boundingBox();
    if (!h || !t) throw new Error("no boxes");
    const sx = h.x + h.width / 2;
    const sy = h.y + h.height / 2;
    const tx = t.x + Math.min(t.width / 2, 20);
    const ty = t.y + t.height / 2;
    await touch(cdp, "touchStart", sx, sy);
    await page.waitForTimeout(450);
    for (let i = 1; i <= 8; i++) {
      await touch(cdp, "touchMove", sx + ((tx - sx) * i) / 8, sy + ((ty - sy) * i) / 8);
    }
    await touch(cdp, "touchEnd");
    await expect(bar(page, "Breach")).toHaveClass(/depth-1/);
    await expect.poll(() => eventParent(page, bookId, ids.get("Breach")!)).toBe(ids.get("Siege"));
    await expect(page.locator(".note-drag-ghost")).toBeHidden();
    await expect(page.locator("#tl-ribbon-drop")).not.toHaveClass(/is-armed/);

    // A tap on a bar opens it — a tap is not a drag.
    await barHit(page, "Parley").scrollIntoViewIfNeeded();
    const p = await barHit(page, "Parley").boundingBox();
    if (!p) throw new Error("no bar");
    await touch(cdp, "touchStart", p.x + p.width / 2, p.y + p.height / 2);
    await touch(cdp, "touchEnd");
    await expect(page.locator(".note-editor-topbar-left")).toContainText("Parley");

    await ctx.close();
  });
});
