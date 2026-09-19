import { test, expect, Page, CDPSession } from "@playwright/test";
import { openNotesPane, registerNewUser } from "./helpers";

/**
 * The notes timeline's phone rendering — a vertical spine (notes revamp, card 7).
 *
 * What is guarded here:
 *
 * 1. **The breakpoint is CSS, not Rust.** `.tl` and `.tl-spine` are both always
 *    mounted (`panes/notes.rs`); the existing `@media (max-width: 768px)` rule in
 *    `pages/book/css.rs` decides which one is visible. At 1280 the spine is present
 *    in the DOM but hidden.
 * 2. **Nesting is indentation**, following `event_parent` exactly as the ribbon does.
 * 3. **The span rule's effect is a text badge** where the ribbon would draw a marker —
 *    "cut at parent" under Clamp.
 * 4. **Simultaneity is an explicit "at the same time" group**, not a geometric
 *    accident: two events that overlap without one nesting in the other.
 * 5. **A relative note sits inline** next to the event it is placed against, and a
 *    `$ref`-only undated note falls into a trailing "not yet dated" group.
 * 6. **The shared filter and selection apply** exactly as they do to the tree and the
 *    desktop timeline.
 * 7. **Tapping a card opens its note** through `open_note_row` (an exit that saves),
 *    including through a real touch scroll — the shape that has broken on touch here
 *    before (rinch fires `onclick` on pointerdown).
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

/** A body whose text is `text` — the server derives #/@/$ edges from it on write. */
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

async function setSpanRule(page: Page, bookId: string, rule: "fit" | "clamp" | "free") {
  const put = await page.request.put(`/api/books/${bookId}`, { data: { span_rule: rule } });
  if (!put.ok()) throw new Error(`set span_rule: ${put.status()}`);
}

/**
 * The siege (1206–1207, clamped) holding a breach that runs to 1210 (past it, so
 * Clamp cuts it and marks it); the parley and Maera's move, both at 1220 — well clear
 * of the siege's own span, so pairing them tests overlap alone, not an accidental
 * three-way merge with the siege — and neither nested in the other, so they pair;
 * "Aftermath", relative to the siege, with no span of its own; and "Letter", undated,
 * naming Vess only through a `$ref` — the not-yet-dated group.
 */
async function seedSpine(page: Page, title: string) {
  const bookId = await seedBook(page, title);
  await setSpanRule(page, bookId, "clamp");
  const ids = await seedNotes(page, bookId, [
    { title: "Vess", patch: () => ({ is_entity: true }) },
    { title: "Corin", patch: () => ({ is_entity: true }) },
    { title: "Maera", patch: () => ({ is_entity: true }) },
    {
      title: "Siege",
      patch: () => ({ span: years(1206, 1207), content: body("$Corin and $Vess #siege ") }),
    },
    {
      title: "Breach",
      patch: (ids) => ({
        span: years(1207, 1210),
        content: body("$Vess "),
        event_parent: ids.get("Siege"),
      }),
    },
    { title: "Parley", patch: () => ({ span: years(1220), content: body("$Vess ") }) },
    { title: "Maera takes the Needle", patch: () => ({ span: years(1220), content: body("$Maera ") }) },
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
}

function spineCard(page: Page, title: string) {
  return page.locator(`.spine-card[data-title="${title}"]`);
}

function looseChip(page: Page, title: string) {
  return page.locator(`.spine-loose-chip[data-title="${title}"]`);
}

test("at 1280 the spine is mounted but hidden, and the ribbon drawing is what's on screen", async ({
  page,
}) => {
  await registerNewUser(page);
  const { bookId } = await seedSpine(page, "Desktop Unchanged Novel");
  await page.goto(`/book/${bookId}`);
  await openTimeline(page);
  await expect(page.locator("#notes-timeline")).toBeVisible();
  await expect(page.locator("#notes-timeline-spine")).toBeAttached();
  await expect(page.locator("#notes-timeline-spine")).toBeHidden();
  // The desktop drawing draws its usual bars — unaffected by the spine's existence:
  // the siege, the breach, the parley and Maera's move all have their own spans.
  await expect(page.locator("#notes-timeline .tl-event")).toHaveCount(4);
});

test.describe("phone", () => {
  test("nesting, a clamp badge, a simultaneous pair, an inline relative note, and a not-yet-dated group", async ({
    browser,
  }) => {
    const ctx = await browser.newContext({
      viewport: { width: 390, height: 780 },
      hasTouch: true,
      isMobile: true,
    });
    const page = await ctx.newPage();
    panics = watchPanics(page);
    await registerNewUser(page);
    const { bookId } = await seedSpine(page, "Phone Spine Novel");
    await page.goto(`/book/${bookId}`);
    await openTimeline(page);

    // The ribbon drawing is hidden; the spine is what's on screen.
    await expect(page.locator("#notes-timeline")).toBeHidden();
    await expect(page.locator("#notes-timeline-spine")).toBeVisible();

    // The page body never scrolls sideways, and the spine needs no horizontal
    // scroller of its own (unlike `.tl-scroll`, which the ribbon drawing needs even
    // at 390px).
    const widths = await page.evaluate(() => ({
      scroll: document.documentElement.scrollWidth,
      client: document.documentElement.clientWidth,
    }));
    expect(widths.scroll).toBeLessThanOrEqual(widths.client);
    // `.tl-scroll` (the ribbon drawing's own horizontal scroller) is still in the DOM
    // — `.tl` is only CSS-hidden, not unmounted — but it takes no space or scroll of
    // its own while hidden.
    await expect(page.locator(".tl-scroll")).toBeHidden();

    // Nesting: the breach is indented under the siege.
    const siegeX = await spineCard(page, "Siege").evaluate((el) => el.getBoundingClientRect().x);
    const breachX = await spineCard(page, "Breach").evaluate((el) => el.getBoundingClientRect().x);
    expect(breachX).toBeGreaterThan(siegeX);

    // Clamp cuts the breach at the siege's edge and marks it — a text badge, since
    // there is no bar here to draw a zigzag on.
    await expect(spineCard(page, "Breach").locator(".spine-card-badge")).toHaveText("cut at parent");
    await expect(spineCard(page, "Siege").locator(".spine-card-badge")).toHaveCount(0);

    // The parley and Maera's move overlap and neither nests in the other: paired.
    const pair = page.locator(".spine-simul", { has: page.locator(".spine-simul-lbl", { hasText: "at the same time" }) });
    await expect(pair).toHaveCount(1);
    await expect(pair.locator(".spine-card")).toHaveCount(2);
    await expect(pair.locator('.spine-card[data-title="Parley"]')).toHaveCount(1);
    await expect(pair.locator('.spine-card[data-title="Maera takes the Needle"]')).toHaveCount(1);

    // Aftermath sits inline, right after the siege's whole subtree, showing the
    // relation as its date and no `$ref` line (it names no one).
    await expect(spineCard(page, "Aftermath").locator(".spine-card-when")).toContainText("after Siege");

    // Order top to bottom: siege, breach (nested under it), the pair, aftermath.
    const order = await page
      .locator(".spine > *")
      .evaluateAll((els) => els.map((e) => e.querySelector("[data-title]")?.getAttribute("data-title") ?? e.className));
    const siegeIdx = order.indexOf("Siege");
    const breachIdx = order.indexOf("Breach");
    const aftermathIdx = order.indexOf("Aftermath");
    expect(siegeIdx).toBeLessThan(breachIdx);
    expect(breachIdx).toBeLessThan(aftermathIdx);

    // Letter has no date and no relation — it names Vess only through `$ref` — so it
    // is in the trailing "not yet dated" group, not on the spine itself.
    await expect(page.locator(".spine-card", { hasText: "Letter" })).toHaveCount(0);
    await expect(page.locator("#spine-loose .spine-loose-heading")).toHaveText("not yet dated");
    await expect(looseChip(page, "Letter")).toBeVisible();
    await expect(looseChip(page, "Letter").locator(".spine-loose-refs")).toContainText("$Vess");

    await ctx.close();
  });

  test("the shared filter and selection apply, exactly as they do to the tree", async ({ browser }) => {
    // Selection has no phone-native trigger yet — the only affordance that sets it
    // (`.tl-chip`, "follow") lives in `.tl`, which is what the phone hides. So this
    // starts at desktop width to set the selection and the filter, then narrows: the
    // point of the test is that the spine reads the *same* `notes_selected` and
    // `notes_filter` signals the tree and the desktop timeline do — a re-render, not
    // a second state to keep in sync — not that the phone offers its own picker.
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const page = await ctx.newPage();
    panics = watchPanics(page);
    await registerNewUser(page);
    const { bookId } = await seedSpine(page, "Phone Filter Novel");
    await page.goto(`/book/${bookId}`);
    await openTimeline(page);
    await expect(page.locator("#notes-timeline")).toBeVisible();

    await page.locator(".tl-chip", { hasText: "Corin" }).click();
    await expect(page.locator(".tl-chip.is-on")).toHaveText("Corin");

    await page.setViewportSize({ width: 390, height: 780 });
    await expect(page.locator("#notes-timeline-spine")).toBeVisible();

    // Corin is in the siege but not the breach or the parley: only the siege lights.
    await expect(spineCard(page, "Siege")).toHaveClass(/is-on/);
    await expect(spineCard(page, "Breach")).not.toHaveClass(/is-on/);
    await expect(spineCard(page, "Parley")).not.toHaveClass(/is-on/);

    // The #siege tag narrows to the siege alone. The breach does not carry the tag
    // itself, and (unlike an ancestor of a match, which is kept and muted) a
    // non-matching *descendant* is simply dropped, so it disappears rather than
    // showing muted.
    await page.locator(".fchip", { has: page.locator(".lbl", { hasText: /^#siege$/ }) }).click();
    await expect(page.locator(".spine-card")).toHaveCount(1);
    await expect(spineCard(page, "Siege")).toBeVisible();
    await expect(spineCard(page, "Siege")).not.toHaveClass(/is-muted/);

    await ctx.close();
  });

  async function touch(cdp: CDPSession, type: string, x?: number, y?: number) {
    await cdp.send("Input.dispatchTouchEvent", {
      type,
      touchPoints: x === undefined ? [] : [{ x, y }],
    });
  }

  test("scrolls under a finger and a tap still opens the card under it", async ({ browser }) => {
    // Short viewport so a modest cast overflows the pane, and a real touch scroll —
    // rinch fires `onclick` on pointerdown, which is exactly what broke a scrollable
    // list of tappable rows before (see `notes-tree.spec.ts`'s twin of this test).
    const ctx = await browser.newContext({
      viewport: { width: 390, height: 500 },
      hasTouch: true,
      isMobile: true,
    });
    const page = await ctx.newPage();
    panics = watchPanics(page);
    const cdp = await ctx.newCDPSession(page);
    await registerNewUser(page);
    const bookId = await seedBook(page, "Touch Spine Novel");
    await seedNotes(
      page,
      bookId,
      // Zero-padded so "Event 2" cannot also match "Event 20".
      Array.from({ length: 20 }, (_, i) => ({
        title: `Event ${String(i + 1).padStart(2, "0")}`,
        patch: () => ({ span: years(1200 + i) }),
      })),
    );
    await page.goto(`/book/${bookId}`);
    await openTimeline(page);
    await expect(spineCard(page, "Event 01")).toBeVisible();

    const pane = page.locator(".notes-pane");
    const overflows = await pane.evaluate((el) => el.scrollHeight > el.clientHeight + 4);
    expect(overflows, "the spine must actually overflow for this to mean anything").toBe(true);

    // ── Drag: scrolls the pane, opens nothing. ──
    const box = await spineCard(page, "Event 03").boundingBox();
    if (!box) throw new Error("no card box");
    const x = box.x + box.width / 2;
    const y0 = box.y + box.height / 2;
    await touch(cdp, "touchStart", x, y0);
    for (let dy = 20; dy <= 200; dy += 20) {
      await touch(cdp, "touchMove", x, y0 - dy);
    }
    await touch(cdp, "touchEnd");

    await expect.poll(() => pane.evaluate((el) => el.scrollTop)).toBeGreaterThan(10);
    await expect(page.locator(".note-editor-topbar-left")).toBeHidden();

    // ── Tap: opens the card under the finger, once any fling has settled. ──
    await expect
      .poll(async () => {
        const a = await pane.evaluate((el) => el.scrollTop);
        await page.waitForTimeout(120);
        return a === (await pane.evaluate((el) => el.scrollTop));
      })
      .toBe(true);
    await pane.evaluate((el) => {
      el.scrollTop = 0;
    });
    await expect.poll(() => pane.evaluate((el) => el.scrollTop)).toBe(0);

    const tapBox = await spineCard(page, "Event 02").boundingBox();
    if (!tapBox) throw new Error("no card box (tap)");
    const tx = tapBox.x + tapBox.width / 2;
    const ty = tapBox.y + tapBox.height / 2;
    await touch(cdp, "touchStart", tx, ty);
    await touch(cdp, "touchEnd");

    await expect(page.locator(".note-editor-topbar-left")).toContainText("Event 02");

    await ctx.close();
  });
});
