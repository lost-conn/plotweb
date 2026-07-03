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
 *
 * NB: a chapter row's up/down `disabled` state (`i == 0` / `i == len - 1`) is
 * evaluated per row when that row is first rendered and is NOT re-evaluated as
 * more chapters are appended in the same session — so after three incremental
 * adds every down arrow is left stale-disabled. Reloading the book renders the
 * whole list once against the final length, giving correct enabled/disabled
 * states, so we reload before driving the pane.
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

  // Reload so the Chapters pane renders once against the final length (fixing the
  // stale per-row `disabled` states left by the incremental adds).
  await page.goto(`/book/${bookId}`);
  await expect(page.locator(".chapter-item").first()).toBeVisible();

  await expect.poll(() => chapterOrder(page)).toEqual(["One", "Two", "Three"]);

  // The top chapter's up arrow (first ActionIcon in its actions) is disabled —
  // "One" can't move above itself.
  const firstUp = page
    .locator(".chapter-item")
    .first()
    .locator(".chapter-item-actions .rinch-action-icon")
    .nth(0);
  await expect(firstUp).toBeDisabled();

  // Click the DOWN arrow (second ActionIcon) on the "One" row => One moves below
  // Two.
  const oneRow = page.locator(".chapter-item", {
    has: page.locator(".chapter-item-left", { hasText: "One" }),
  });
  await oneRow.locator(".chapter-item-actions .rinch-action-icon").nth(1).click();

  // The reorder lands after the optimistic swap + reorder PUT — auto-wait.
  await expect.poll(() => chapterOrder(page)).toEqual(["Two", "One", "Three"]);

  // Reload proves the new order reached the server (not just optimistic UI).
  await page.goto(`/book/${bookId}`);
  await expect.poll(() => chapterOrder(page)).toEqual(["Two", "One", "Three"]);
});
