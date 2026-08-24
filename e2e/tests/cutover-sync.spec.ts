import { test, expect, Browser, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * A cut-over book, with sync on — the configuration production runs.
 *
 * Every other spec runs with cutover off, because a test creates its book at runtime
 * and there is no id to name in `PLOTWEB_CUTOVER_BOOKS` at boot. So these run against a
 * server started with the wildcard:
 *
 *     cd e2e && npm run test:cutover
 *
 * Worth the separate invocation: this combination is where every production bug in this
 * arc has lived, and none of them were reachable from the default suite.
 */

async function openDevice(browser: Browser, baseURL: string): Promise<Page> {
  const context = await browser.newContext();
  await context.addInitScript(() => {
    window.localStorage.setItem("plotweb_sync", "1");
  });
  const page = await context.newPage();
  await page.goto(baseURL);
  return page;
}

/** Chapter titles as the sidebar renders them, in order.
 *
 * A row reads "1The Adventure Begins0 words" — index badge, title, word count — so the
 * title is what is left after stripping those. Worth the regex: the duplication this
 * file exists for shows up as the same title twice, and a looser assertion would miss
 * exactly that. */
async function sidebar(page: Page): Promise<string[]> {
  const rows = await page.locator(".chapter-item .chapter-item-left").allTextContents();
  return rows.map((r) => r.replace(/^\d+/, "").replace(/\d+ words$/, ""));
}

/** How many rows carry this title — 2 is the bug. */
function rowsTitled(page: Page, title: string) {
  return page.locator(".chapter-item", {
    has: page.locator(".chapter-item-left", { hasText: title }),
  });
}

test("a chapter appears once in the sidebar, not twice", async ({ browser, baseURL }) => {
  // Reported from production: three chapters created, six rows rendered, each title
  // twice — and both rows highlighting together, so it is one id drawn twice rather
  // than two chapters. The empty second copy is `project_chapters` materialising a
  // placeholder for an id it has already consumed.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Cutover Sync Novel");

  await addChapter(page, "The Adventure Begins");
  await addChapter(page, "The Adventure Continues");
  await addChapter(page, "The Adventure Concludes");

  // Let the local doc's dual-write, the REST apply and a sync round all land.
  await page.waitForTimeout(8000);

  const expected = [
    "The Adventure Begins",
    "The Adventure Continues",
    "The Adventure Concludes",
  ];
  expect(await sidebar(page)).toEqual(expected);
  for (const title of expected) {
    await expect(rowsTitled(page, title)).toHaveCount(1);
  }

  // And it survives a reload, which is when the local doc is re-projected.
  await page.reload();
  await expect(page.locator(".chapter-item").first()).toBeVisible();
  expect(await sidebar(page)).toEqual(expected);
  for (const title of expected) {
    await expect(rowsTitled(page, title)).toHaveCount(1);
  }
});

test("deleting and re-adding chapters does not leave duplicates", async ({
  browser,
  baseURL,
}) => {
  // The reporter's actual sequence: they deleted and re-added the chapters partway
  // through, to re-test. A delete that reaches one copy and not the other, followed by
  // a create, is the most likely way to end up with a list holding an id twice.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Redo Novel");

  await addChapter(page, "One");
  await addChapter(page, "Two");
  await page.waitForTimeout(6000);

  // Delete both, through the row's delete action (third action-icon).
  for (const title of ["Two", "One"]) {
    const row = page.locator(".chapter-item", {
      has: page.locator(".chapter-item-left", { hasText: title }),
    });
    // Actions are [up, down, rename, delete]; the red trash is the fourth.
    await row.locator(".chapter-item-actions .rinch-action-icon").nth(3).click();
    const modal = page.locator(".rinch-modal__body:visible");
    if (await modal.count()) {
      await modal.getByRole("button", { name: /Delete|Confirm/ }).click();
    }
    await expect(row).toHaveCount(0);
  }
  await page.waitForTimeout(6000);

  await addChapter(page, "One");
  await addChapter(page, "Two");
  await page.waitForTimeout(8000);

  expect(await sidebar(page)).toEqual(["One", "Two"]);
  await page.reload();
  await expect(page.locator(".chapter-item").first()).toBeVisible();
  expect(await sidebar(page)).toEqual(["One", "Two"]);
  await expect(rowsTitled(page, "One")).toHaveCount(1);
  await expect(rowsTitled(page, "Two")).toHaveCount(1);
});

test("an edit to a chapter body survives a reload", async ({ browser, baseURL }) => {
  // Reported after the duplication was fixed: "they're still losing my changes".
  //
  // This is the configuration where the two halves meet. The client declares
  // `sync_owned`, so the server deliberately drops the REST body write and leaves the
  // canonical document to sync. The read then comes from that canonical document. If
  // sync does not actually land the edit there, the write has been suppressed in favour
  // of a writer that never wrote — and the text is gone on the next load.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Body Persist Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "This sentence has to be here after a reload.");
  await page.waitForTimeout(8000);

  await page.reload();
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText(
    "This sentence has to be here after a reload.",
  );
});

test("a second edit to the same chapter also survives", async ({ browser, baseURL }) => {
  // Once a body is registered with the sync engine the REST write stops, so the second
  // edit exercises a different path from the first — the first may still have been
  // written over REST before registration completed.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Second Edit Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "First sentence.");
  await page.waitForTimeout(8000);
  await typeInEditor(page, " Second sentence, typed once sync owns this body.");
  await page.waitForTimeout(8000);

  await page.reload();
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText("First sentence.");
  await expect(page.locator("#editor-main")).toContainText(
    "Second sentence, typed once sync owns this body.",
  );
});

test("switching between chapters does not empty them", async ({ browser, baseURL }) => {
  // Reported: text vanishes on switching chapters, only sometimes, and "unsaved" flashes
  // up as the chapter loads before it goes. That status comes from
  // `schedule_chapter_autosave`, so the *load* is being recorded as an edit — and with
  // sync owning the body that edit goes straight into the CRDT and up to the canonical
  // copy. What gets recorded is the editor's state before the content lands, so the
  // document is overwritten with nothing. Word count 0, "saved" perfectly truthful.
  //
  // The existing crosstalk spec covers this shape with cutover off, where the damage is
  // two chapters mixing. Under cutover it is destruction, not confusion.
  test.setTimeout(180_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Switching Novel");
  await addChapter(page, "One");
  await addChapter(page, "Two");
  await addChapter(page, "Three");

  const text: Record<string, string> = {
    One: "Prose belonging to chapter one alone.",
    Two: "Prose belonging to chapter two alone.",
    Three: "Prose belonging to chapter three alone.",
  };

  for (const [title, prose] of Object.entries(text)) {
    await openChapter(page, title);
    await typeInEditor(page, prose);
    await page.waitForTimeout(4000);
  }

  // Switch around, sometimes pausing and sometimes not — the report says it happens
  // "only sometimes", which points at a race rather than a rule.
  const order = ["One", "Three", "Two", "One", "Two", "Three", "One", "Three", "Two"];
  for (const [i, title] of order.entries()) {
    await openChapter(page, title);
    await page.waitForTimeout(i % 2 === 0 ? 300 : 2500);
  }
  await page.waitForTimeout(8000);

  await page.reload();
  for (const [title, prose] of Object.entries(text)) {
    await openChapter(page, title);
    await expect(
      page.locator("#editor-main"),
      `chapter ${title} must still hold its own prose`,
    ).toContainText(prose, { timeout: 15_000 });
  }
});
