import { test, expect, Browser, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * A syncing client must not *also* save over REST.
 *
 * This is the co-existence rule from `docs/sync-engine-design.md`, and it was written
 * down long before it was enforced. What happens without it, in production, is that
 * deleted text comes back under the author's cursor: sync carries the deletion across
 * as an incremental change, while the 3-second REST autosave PUTs a whole snapshot of
 * the editor model taken a moment earlier. The server applies that snapshot into the
 * canonical document as a diff, and the diff says "re-insert the sentence".
 *
 * The test watches the network rather than the prose. Asserting on rendered text would
 * need the reinstatement race to actually land, which is timing-dependent and would
 * pass by luck on a quiet machine; "did this client send a body write at all" is the
 * property, and it is exact.
 */

/** A device: a fresh context with sync enabled before any script runs. */
async function openDevice(browser: Browser, baseURL: string): Promise<Page> {
  const context = await browser.newContext();
  await context.addInitScript(() => {
    window.localStorage.setItem("plotweb_sync", "1");
  });
  const page = await context.newPage();
  await page.goto(baseURL);
  return page;
}

test("sync: a synced chapter body is not also written over REST", async ({ browser, baseURL }) => {
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Single Writer Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  // Let the body register with the sync engine and establish provenance before we
  // start judging its writes.
  await typeInEditor(page, "First sentence, to get sync going.");
  await page.waitForTimeout(6000);

  const bodyWrites: string[] = [];
  page.on("request", async (r) => {
    if (r.method() !== "PUT") return;
    if (!/\/api\/books\/[^/]+\/chapters\//.test(r.url())) return;
    // A title-only PUT is fine — titles are structure, and sync does not carry them.
    // A PUT carrying `content` is the second writer this test exists to forbid.
    const body = r.postData() ?? "";
    if (body.includes('"content"') && !body.includes('"content":null')) {
      bodyWrites.push(new URL(r.url()).pathname);
    }
  });

  await typeInEditor(page, " Second sentence, typed while sync is on.");
  await page.waitForTimeout(6000);

  expect(
    bodyWrites,
    `sync owns this body; REST must not write it too: ${bodyWrites}`,
  ).toEqual([]);

  // And the edit is not lost by dropping that write — it is durable through sync and
  // the local store, which is the whole reason dropping it is safe.
  await page.reload();
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText(
    "Second sentence, typed while sync is on.",
  );
});

test("sync off: the chapter body is still written over REST", async ({ page }) => {
  // The counterpart. Sync is off by default, and then REST is the only writer there
  // is — a guard that silenced it unconditionally would lose every save.
  await registerNewUser(page);
  await createBook(page, "Rest Writer Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  const bodyWrites: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/chapters\//.test(r.url())) {
      const body = r.postData() ?? "";
      if (body.includes('"content"')) bodyWrites.push(new URL(r.url()).pathname);
    }
  });

  await typeInEditor(page, "Written with sync off.");
  await page.waitForTimeout(6000);

  expect(bodyWrites.length, "REST is the only writer when sync is off").toBeGreaterThan(0);
});
