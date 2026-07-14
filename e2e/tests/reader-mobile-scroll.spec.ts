import { test, expect } from "@playwright/test";
import { createBetaLink, createBook, registerNewUser } from "./helpers";

/**
 * Mobile reader sidebar: the chapter-list drawer must SCROLL under a finger drag
 * without the drag being mistaken for a chapter selection.
 *
 * rinch dispatches `onclick` on `pointerdown` (for immediate drag arming). That
 * made a list of clickable rows inside an `overflow: auto` container un-scrollable
 * on touch — the first contact of a scroll gesture fired a row's click, opening a
 * chapter and closing the drawer before the pan was recognised. The rinch fix
 * defers a touch/pen tap's click to `pointerup` when the target has a scrollable
 * ancestor, and abandons it if the contact scrolls past the slop. This test drives
 * real touch events (via CDP) to prove: a drag scrolls (no selection), a tap opens.
 */

const READER_ORIGIN = "http://localhost:3000";

test("mobile reader: chapter list scrolls under a drag, taps still open a chapter", async ({
  browser,
}) => {
  // Author: a book with enough chapters to overflow the mobile drawer's list.
  const setup = await browser.newContext({ baseURL: READER_ORIGIN });
  const author = await setup.newPage();
  await registerNewUser(author);
  const bookId = await createBook(author, "Mobile Scroll Novel");
  for (let i = 1; i <= 15; i++) {
    const resp = await author.request.post(`/api/books/${bookId}/chapters`, {
      data: { title: `Chapter ${i}` },
    });
    if (!resp.ok()) throw new Error(`create chapter ${i}: ${resp.status()}`);
  }
  const token = await createBetaLink(author, bookId, "MobileScroller");

  // Reader on a short touch viewport so the chapter list is forced to scroll.
  const ctx = await browser.newContext({
    baseURL: READER_ORIGIN,
    viewport: { width: 390, height: 480 },
    hasTouch: true,
    isMobile: true,
  });
  const reader = await ctx.newPage();
  const cdp = await ctx.newCDPSession(reader);
  await reader.goto(`/read/${token}`);

  // Open the sidebar drawer (hamburger = first mobile-topbar icon).
  await reader.locator(".reader-mobile-topbar .rinch-action-icon").nth(0).click();
  const list = reader.locator(".reader-sidebar-chapters");
  await expect(list).toBeVisible();
  await expect(reader.locator(".reader-chapter-item").first()).toBeVisible();
  // No chapter is open yet (fresh beta link → welcome pane, no reading column).
  await expect(reader.locator("#reader-content")).toHaveCount(0);

  // Precondition: the list actually overflows (a scroll is possible/needed).
  const overflows = await list.evaluate((el) => el.scrollHeight > el.clientHeight + 4);
  expect(overflows).toBe(true);

  // ── Drag gesture: a vertical drag starting ON a chapter row must SCROLL the
  //    list, not select a chapter (the regression). ──────────────────────────
  const box = await reader.locator(".reader-chapter-item").nth(1).boundingBox();
  if (!box) throw new Error("no chapter item box");
  const x = box.x + box.width / 2;
  const y0 = box.y + box.height / 2;

  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x, y: y0 }],
  });
  for (let dy = 20; dy <= 220; dy += 20) {
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [{ x, y: y0 - dy }],
    });
  }
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  // The list scrolled…
  await expect
    .poll(() => list.evaluate((el) => el.scrollTop))
    .toBeGreaterThan(10);
  // …and no chapter opened: drawer still open, still on the welcome pane.
  await expect(reader.locator(".reader-sidebar.open")).toBeVisible();
  await expect(reader.locator("#reader-content")).toHaveCount(0);

  // ── Tap gesture: a stationary tap on a chapter row DOES open it. ───────────
  await list.evaluate((el) => {
    el.scrollTop = 0;
  });
  const tapBox = await reader.locator(".reader-chapter-item").first().boundingBox();
  if (!tapBox) throw new Error("no chapter item box (tap)");
  const tx = tapBox.x + tapBox.width / 2;
  const ty = tapBox.y + tapBox.height / 2;
  await cdp.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x: tx, y: ty }],
  });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  // Chapter opened: the reading column mounts and the drawer closes.
  await expect(reader.locator("#reader-content")).toBeVisible();
  await expect(reader.locator(".reader-sidebar.open")).toHaveCount(0);

  await ctx.close();
  await setup.close();
});
