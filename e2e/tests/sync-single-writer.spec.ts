import { test, expect, Browser, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * One document, one writer — decided by whether the book is cut over.
 *
 * Two writers on one body is how deleted text comes back under the author's cursor:
 * sync carries the deletion across as an incremental change while the REST autosave
 * PUTs a whole snapshot of the editor model taken a moment earlier, and the server
 * applies that snapshot into the canonical document as a diff that says "re-insert the
 * sentence".
 *
 * There used to be a `sync_owned` declaration on the wire, and a negotiation between
 * two writers over which should stand down. It is gone: for a cut-over book sync is the
 * only writer of a body, and everywhere else REST is. What remains to protect is the
 * regression the negotiation was introduced to fix — a client withholding the body
 * write for a book that is *not* cut over, where git is the truth and that write is the
 * only thing reaching it. The edit landed in the canonical store, never reached git,
 * and vanished on the next read.
 *
 * Cutover itself is covered server-side (`tests/cutover.rs::one_writer`), where a book
 * can actually be cut over; here the server is never cut over, so these pin the
 * everywhere-else half.
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

test("sync on, book not cut over: edits still persist across a reload", async ({
  browser,
  baseURL,
}) => {
  // The regression this exists for. The single-writer rule was first enforced by having
  // the client withhold the REST write whenever a body was syncing — but *which* writer
  // should stand down depends on whether the book is cut over, and the client cannot
  // know that. For every book that was not cut over (which is all of them, by default)
  // the edit went into the canonical store, never reached git, and vanished on the next
  // read. The author's description was "my edits aren't sticking".
  //
  // The e2e server cuts nothing over, so this is exactly that configuration.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Not Cut Over Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "This has to survive a reload.");
  await page.waitForTimeout(6000);

  await page.reload();
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText("This has to survive a reload.");
});

test("sync on, book not cut over: the body write still carries content", async ({
  browser,
  baseURL,
}) => {
  // The regression this file exists for. Sync being on must not make the client
  // withhold the body: git is the truth for a book that is not cut over, and this write
  // is the only thing that reaches it.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Declaring Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  // Let the body register with the sync engine before judging its writes.
  await typeInEditor(page, "First sentence, to get sync going.");
  await page.waitForTimeout(6000);

  const carried: boolean[] = [];
  page.on("request", (r) => {
    if (r.method() !== "PUT") return;
    if (!/\/api\/books\/[^/]+\/chapters\//.test(r.url())) return;
    const body = r.postData() ?? "";
    if (!body.includes('"content"')) return;
    carried.push(!body.includes('"content":null'));
    expect(body, "the declaration is gone from the wire").not.toContain("sync_owned");
  });

  await typeInEditor(page, " Second sentence, typed while sync is on.");
  await page.waitForTimeout(6000);

  expect(carried.length, "the body write must still be sent").toBeGreaterThan(0);
  expect(
    carried.every(Boolean),
    `every body write must carry content here: ${JSON.stringify(carried)}`,
  ).toBe(true);
});

test("sync off: the chapter body is written over REST", async ({ page }) => {
  // The counterpart. Sync is off and the book is not cut over, so REST is the only
  // writer there is.
  await registerNewUser(page);
  await createBook(page, "Rest Writer Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  const bodyWrites: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/chapters\//.test(r.url())) {
      const body = r.postData() ?? "";
      if (!body.includes('"content"') || body.includes('"content":null')) return;
      expect(body, "the declaration is gone from the wire").not.toContain("sync_owned");
      bodyWrites.push(new URL(r.url()).pathname);
    }
  });

  await typeInEditor(page, "Written with sync off.");
  await page.waitForTimeout(6000);

  expect(bodyWrites.length, "REST is the only writer when sync is off").toBeGreaterThan(0);
});
