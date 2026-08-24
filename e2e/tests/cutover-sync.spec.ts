import { test, expect, Browser, Page } from "@playwright/test";
import { addChapter, createBook, registerNewUser } from "./helpers";

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
