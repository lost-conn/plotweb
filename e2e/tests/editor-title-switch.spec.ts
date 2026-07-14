import { test, expect } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser } from "./helpers";

/**
 * Switching chapters after EDITING a chapter title must show the newly-opened
 * chapter's title in the inline title field — not the edited previous title.
 *
 * The inline title is a rinch TextInput bound with `value_fn`. rinch's web
 * backend updated the field via setAttribute("value", …), which the browser
 * ignores once the field is "dirty" (the user has typed in it) — so after
 * editing a title, a chapter switch left the stale title in the box. The rinch
 * fix also drives the live `.value` property.
 */

const titleInput = ".editor-title-input input";

test("editor: switching chapters after editing a title updates the title field", async ({ page }) => {
  await registerNewUser(page);
  await createBook(page, "Title Switch Novel");
  await addChapter(page, "Alpha");
  await addChapter(page, "Beta");

  // Open Alpha in the editor; the title field shows "Alpha".
  await openChapter(page, "Alpha");
  await expect(page.locator(titleInput)).toHaveValue("Alpha");

  // Edit the title (this makes the input "dirty").
  await page.locator(titleInput).fill("Alpha EDITED");
  await expect(page.locator(titleInput)).toHaveValue("Alpha EDITED");

  // Switch to Beta — the field must now show Beta's title, not the edited one.
  await openChapter(page, "Beta");
  await expect(page.locator(titleInput)).toHaveValue("Beta");

  // And Beta's field is itself editable + reactive after the switch.
  await page.locator(titleInput).fill("Beta EDITED");
  await expect(page.locator(titleInput)).toHaveValue("Beta EDITED");
  await openChapter(page, "Alpha");
  await expect(page.locator(titleInput)).toHaveValue("Alpha");
});
