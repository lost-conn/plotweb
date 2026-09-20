import { test, expect } from "@playwright/test";
import { registerNewUser } from "./helpers";

/**
 * The dashboard shelf scrolls when it overflows the viewport.
 *
 * The dashboard's route wrapper (app_shell.rs) is a 100dvh flex column with
 * `overflow: hidden`, and `.dash-body` is meant to be the `flex: 1;
 * overflow-y: auto` child that scrolls. But the component mounts inside an
 * unstyled block container between the two, so `.dash-body` never had a
 * constrained height: it grew to its content, and the wrapper clipped the
 * shelf. With more books than fit, the bottom rows were simply unreachable —
 * no scrollbar, wheel did nothing. The page now owns a `100dvh` flex root
 * (`.dash-page`), like the book page's `.book-workspace`.
 */

const SHELF = ".dash-body";

test("the shelf scrolls when there are more books than fit", async ({ page, request }) => {
  // A short viewport so a handful of jackets is already too many.
  await page.setViewportSize({ width: 900, height: 480 });
  await registerNewUser(page);

  // Seed straight through the API; the UI path is covered elsewhere and would
  // open each book on the way.
  for (let i = 1; i <= 8; i++) {
    const resp = await page.request.post("/api/books", {
      data: { title: `Shelf Book ${i}`, description: "" },
    });
    expect(resp.ok(), `create book ${i}: ${resp.status()}`).toBe(true);
  }
  await page.reload();
  await expect(page.locator(".shelf .bk")).toHaveCount(8);

  // Precondition: the shelf really overflows the body at this height.
  const body = page.locator(SHELF);
  await expect
    .poll(() => body.evaluate((el) => el.scrollHeight - el.clientHeight))
    .toBeGreaterThan(100);

  // The body is the scroller: moving its scrollTop moves the content, and the
  // "New book" card — always the shelf's final row, whatever order the
  // jackets sort into — comes into the body's viewport.
  await body.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await expect.poll(() => body.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  const last = page.locator(".shelf .bk-new");
  const inside = await last.evaluate((el, sel) => {
    const r = el.getBoundingClientRect();
    const s = document.querySelector(sel)!.getBoundingClientRect();
    return r.top >= s.top - 1 && r.bottom <= s.bottom + 1;
  }, SHELF);
  expect(inside).toBe(true);

  // A real wheel gesture over the shelf scrolls it too (not just a script).
  await body.evaluate((el) => {
    el.scrollTop = 0;
  });
  const box = await body.boundingBox();
  if (!box) throw new Error("no box for the shelf body");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, 400);
  await expect.poll(() => body.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);

  // The topbar stays put above the scrolling body.
  await expect(page.locator(".dash-topbar")).toBeVisible();
});
