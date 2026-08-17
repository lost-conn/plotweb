import { test, expect } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * Merely *looking* at a document must never write it.
 *
 * Save-on-leave used to fire unconditionally, so opening a chapter or note and
 * navigating away rewrote it with whatever the editor happened to hold. Harmless
 * while the editor holds what is stored — and destructive the moment it doesn't.
 * A note lost a paragraph in production exactly this way: it was opened during a
 * cutover, showed the (blank) canonical copy of a document that disagreed with git,
 * and walking away wrote that blank over git's copy.
 *
 * The test can't easily stage a divergence through the UI, so it pins the rule that
 * makes divergence survivable: no edit, no write. It watches the network rather than
 * the rendered text, because "the content still looks right" would also pass if the
 * app wrote identical bytes back — and a write that is harmless today is the bug that
 * bites tomorrow.
 */
test("editor: opening a chapter and leaving it writes nothing", async ({ page }) => {
  await registerNewUser(page);
  await createBook(page, "Read Only Novel");
  await addChapter(page, "Alpha");
  await addChapter(page, "Beta");

  await openChapter(page, "Alpha");
  await typeInEditor(page, "Alpha prose that must survive being looked at.");
  await page.waitForTimeout(4000); // let the autosave land

  // From here on, any PUT to a chapter is a write we did not ask for.
  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/chapters\//.test(r.url())) {
      writes.push(`${r.method()} ${new URL(r.url()).pathname}`);
    }
  });

  // Look at Beta, look back at Alpha, look at Beta again — touching nothing.
  await openChapter(page, "Beta");
  await page.waitForTimeout(1500);
  await openChapter(page, "Alpha");
  await page.waitForTimeout(1500);
  await openChapter(page, "Beta");
  await page.waitForTimeout(4000);

  expect(writes, `no chapter should be written when nothing was edited: ${writes}`).toEqual([]);

  // And the prose is intact, read back from the server.
  await page.reload();
  await openChapter(page, "Alpha");
  await expect(page.locator("#editor-main")).toContainText(
    "Alpha prose that must survive being looked at.",
  );
});

test("editor: an actual edit still saves on the way out", async ({ page }) => {
  await registerNewUser(page);
  await createBook(page, "Still Saves Novel");
  await addChapter(page, "Alpha");
  await addChapter(page, "Beta");

  await openChapter(page, "Alpha");
  await typeInEditor(page, "First pass.");
  // Leave immediately, inside the autosave debounce: the save-on-leave is what
  // catches this, and the dirty guard must not have disabled it.
  await openChapter(page, "Beta");
  await page.waitForTimeout(2000);

  await page.reload();
  await openChapter(page, "Alpha");
  await expect(page.locator("#editor-main")).toContainText("First pass.");
});
