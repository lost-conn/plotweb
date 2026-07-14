import { test, expect } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

test("create a book and see it on the dashboard", async ({ page }) => {
  await registerNewUser(page);
  const title = "The Great Test Novel";
  await createBook(page, title);
  // Back on the dashboard the book is listed.
  await page.goto("/");
  await expect(page.getByText(title, { exact: true }).first()).toBeVisible();
});

test("add a chapter, write content, and it persists across reload", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Persistence Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  const prose = "The quick brown fox jumped over the lazy dog.";
  await typeInEditor(page, prose);

  // Wait past the autosave debounce (3s) so the chapter is PUT to the server.
  await page.waitForTimeout(4000);

  // Full reload — content must come back from the server.
  await page.goto(`/book/${bookId}`);
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText(prose);
});

test("leaving the editor immediately after typing does not lose the edit", async ({ page }) => {
  // Regression for the audit's HIGH finding: navigating out of the editor within
  // the 3s autosave debounce used to silently drop the in-flight edit.
  await registerNewUser(page);
  const bookId = await createBook(page, "No Lost Edits");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  const prose = "An edit made right before navigating away.";
  await typeInEditor(page, prose);

  // Immediately leave the editor pane (no debounce wait) via a sidebar section,
  // which must flush the pending save.
  await page.locator(".sidebar-section-header", { hasText: "TYPOGRAPHY" }).click();
  // Come back to the chapter.
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText(prose);

  // And it actually reached the server: reload proves it.
  await page.goto(`/book/${bookId}`);
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText(prose);
});

test("a second account cannot open another user's book by URL", async ({ page }) => {
  // Create a book as user A.
  await registerNewUser(page);
  const bookId = await createBook(page, "Private Manuscript");

  // Become user B (registering swaps the session) and try the direct URL.
  await registerNewUser(page);
  await page.goto(`/book/${bookId}`);

  // No chapters/sidebar for a book they don't own; the editor is never reachable.
  await expect(page.locator(".sidebar-chapter-name")).toHaveCount(0);
  await expect(page.getByText("Private Manuscript", { exact: true })).toHaveCount(0);
});
