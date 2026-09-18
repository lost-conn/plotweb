import { test, expect, Page } from "@playwright/test";
import { addChapter, createBook, registerNewUser } from "./helpers";

/**
 * Chapter reordering coverage.
 *
 * The Chapters pane (the book's default pane) lists each chapter as a `.crow`
 * row (`panes/chapters.rs`). Reordering is no longer up/down chevrons — it is
 * drag-and-drop by a handle: each row has a `.grip` (`draggable="true"`) that
 * starts the drag (`ondragstart` records the dragged chapter id), and the row
 * itself is the drop target (`ondragover`/`ondrop`, which calls
 * `reorder_chapter` — optimistic swap of `store.chapters` + a PUT to
 * `/api/books/{id}/chapters/reorder`). The swap + network round-trip is async,
 * so every order assertion leans on Playwright auto-waiting.
 *
 * Chapter order is reflected live in the sidebar's `.sidebar-chapter-name` list
 * (title-only, driven by the same `store.chapters` signal), which we read to
 * assert order.
 */

/** Chapter titles in live DOM order, read from the sidebar list. */
async function chapterOrder(page: Page): Promise<string[]> {
  return page.locator(".sidebar-chapter-name").allInnerTexts();
}

/** A chapter row in the chapters pane, found by its title text. */
function row(page: Page, title: string) {
  return page.locator(".chapter-rows .crow", {
    has: page.locator(".t", { hasText: title }),
  });
}

test("reorder chapters and it persists", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Reorder Novel");

  // Chapters append in creation order.
  await addChapter(page, "One");
  await addChapter(page, "Two");
  await addChapter(page, "Three");

  await expect(page.locator(".chapter-rows .crow").first()).toBeVisible();

  await expect.poll(() => chapterOrder(page)).toEqual(["One", "Two", "Three"]);

  // Drag "One"'s grip handle onto the "Two" row => One moves below Two.
  // Playwright's `dragTo` fires a real dragstart/dragover/drop sequence, which
  // rinch's grip/row listeners consume (verified working against this exact
  // markup by hand).
  await row(page, "One").locator(".grip").dragTo(row(page, "Two"));

  // The reorder lands after the optimistic swap + reorder PUT — auto-wait.
  await expect.poll(() => chapterOrder(page)).toEqual(["Two", "One", "Three"]);

  // Reload proves the new order reached the server (not just optimistic UI).
  await page.goto(`/book/${bookId}`);
  await expect.poll(() => chapterOrder(page)).toEqual(["Two", "One", "Three"]);
});
