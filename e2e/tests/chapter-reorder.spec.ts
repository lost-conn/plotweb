import { test, expect, Page } from "@playwright/test";
import { addChapter, createBook, registerNewUser } from "./helpers";

/**
 * Chapter reordering coverage.
 *
 * The Chapters pane (the book's default pane) lists each chapter as a
 * `.chapter-item` card. Its `.chapter-item-actions` holds three ActionIcons in
 * order: up (ChevronUp), down (ChevronDown), delete. Clicking up/down calls
 * `move_chapter`, which optimistically swaps `store.chapters` and PUTs the new
 * order to `/api/books/{id}/chapters/reorder`. The swap + network round-trip is
 * async, so every order assertion leans on Playwright auto-waiting.
 *
 * Chapter order is reflected live in the sidebar's `.sidebar-chapter-name` list
 * (title-only, driven by the same `store.chapters` signal), which we read to
 * assert order.
 */

/** Chapter titles in live DOM order, read from the sidebar list. */
async function chapterOrder(page: Page): Promise<string[]> {
  return page.locator(".sidebar-chapter-name").allInnerTexts();
}

test("reorder chapters and it persists", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Reorder Novel");

  // Chapters append in creation order.
  await addChapter(page, "One");
  await addChapter(page, "Two");
  await addChapter(page, "Three");

  await expect(page.locator(".chapter-item").first()).toBeVisible();

  await expect.poll(() => chapterOrder(page)).toEqual(["One", "Two", "Three"]);

  // Row helpers by title. Actions are [up, down, delete]; up == nth(0),
  // down == nth(1).
  const row = (title: string) =>
    page.locator(".chapter-item", {
      has: page.locator(".chapter-item-left", { hasText: title }),
    });
  const upArrow = (title: string) =>
    row(title).locator(".chapter-item-actions .rinch-action-icon").nth(0);
  const downArrow = (title: string) =>
    row(title).locator(".chapter-item-actions .rinch-action-icon").nth(1);

  // Live disabled/enabled states must be correct WITHOUT reloading, straight
  // after the three in-session appends. This is the regression guard: each time
  // a chapter is appended the previously-last row must re-render so its down
  // arrow stops being stale-disabled.
  //
  //  - "One" (top): can't move up   -> up disabled; can move down   -> down enabled.
  //  - "Two" (middle): can move both ways -> up & down both enabled.
  //  - "Three" (bottom): can't move down -> down disabled; can move up -> up enabled.
  await expect(upArrow("One")).toBeDisabled();
  await expect(downArrow("One")).toBeEnabled();
  await expect(upArrow("Two")).toBeEnabled();
  await expect(downArrow("Two")).toBeEnabled();
  await expect(upArrow("Three")).toBeEnabled();
  await expect(downArrow("Three")).toBeDisabled();

  // Click the DOWN arrow (second ActionIcon) on the "One" row => One moves below
  // Two.
  const oneRow = row("One");
  await oneRow.locator(".chapter-item-actions .rinch-action-icon").nth(1).click();

  // The reorder lands after the optimistic swap + reorder PUT — auto-wait.
  await expect.poll(() => chapterOrder(page)).toEqual(["Two", "One", "Three"]);

  // Reload proves the new order reached the server (not just optimistic UI).
  await page.goto(`/book/${bookId}`);
  await expect.poll(() => chapterOrder(page)).toEqual(["Two", "One", "Three"]);
});
