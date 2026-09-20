import { test, expect, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser } from "./helpers";

/**
 * The sidebar's "Manuscript" header opens the manuscript page (the chapters
 * pane), the way the "Notes" header opens the notes surface.
 *
 * It used to be a bare label: the caret beside it collapsed the chapter list
 * and the "+" added a chapter, but clicking the word did nothing — and from an
 * open chapter there was no other way back to the pane that lists the
 * manuscript with its counts and the import/export menu. rinch dispatches a
 * click to the nearest handler only, so the caret and the "+" keep their own
 * behaviour rather than also firing the header's.
 */

const header = (page: Page) => page.locator(".pw-section-header", { hasText: "Manuscript" });
// `.chapters-pane` is the shared pane chrome (typography, beta readers, history
// and the calendar wear it too); the manuscript page is the one with the
// "Add chapter" control.
const chaptersPane = (page: Page) =>
  page.locator(".chapters-pane", { has: page.getByRole("button", { name: "Add chapter" }) });
const chapterList = (page: Page) => page.locator(".sidebar-chapter-list");

test("clicking Manuscript opens the chapters pane; the caret and + keep their own jobs", async ({
  page,
}) => {
  await registerNewUser(page);
  await createBook(page, "Header Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  // Precondition: the editor is up and the chapters pane is not.
  await expect(page.locator("#editor-main [data-pm-editor]")).toBeVisible();
  await expect(chaptersPane(page)).toBeHidden();

  // The header label itself navigates.
  await header(page).locator(".pw-section-header-label").click();
  await expect(chaptersPane(page)).toBeVisible();
  await expect(page.locator("#editor-main [data-pm-editor]")).toBeHidden();
  await expect(header(page)).toHaveClass(/pw-section-header--active/);

  // The caret only collapses the list; it does not re-navigate (go back to
  // the editor first so a navigation would be observable).
  await openChapter(page, "Chapter One");
  await expect(chaptersPane(page)).toBeHidden();
  const caret = header(page).locator("span", { hasText: /[▸▾]/ }).first();
  await caret.click();
  await expect(chapterList(page)).toBeHidden();
  await expect(chaptersPane(page)).toBeHidden();
  await caret.click();
  await expect(chapterList(page)).toBeVisible();

  // The "+" still opens the inline new-chapter row in the chapters pane.
  await header(page).locator(".rinch-action-icon").click();
  await expect(chaptersPane(page)).toBeVisible();
  await expect(page.locator("#chapter-inline-title")).toBeVisible();
});
