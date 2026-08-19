import { test, expect, Browser, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * One document, one writer — decided by the server, declared by the client.
 *
 * Two writers on one body is how deleted text comes back under the author's cursor:
 * sync carries the deletion across as an incremental change while the REST autosave
 * PUTs a whole snapshot of the editor model taken a moment earlier, and the server
 * applies that snapshot into the canonical document as a diff that says "re-insert the
 * sentence".
 *
 * But *which* writer should stand down depends on whether the book is cut over, and
 * neither side knows both halves: the client knows whether it is syncing this document,
 * the server knows whether the book's reads come from the canonical copy. The first
 * attempt at this rule had the client withhold the write on its own, which dropped the
 * save for every book that was not cut over — the edit reached the canonical store,
 * never reached git, and vanished on the next read.
 *
 * So the client always writes and declares `sync_owned`, and the server drops the body
 * only where that declaration means something. These cover both halves: the write still
 * lands when the book is not cut over (the regression), and it still carries the
 * declaration so the server has something to decide with (the original bug).
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

test("sync on: the body write declares that sync owns it", async ({ browser, baseURL }) => {
  // The declaration is what lets the *server* decide, since only it knows whether the
  // book is cut over. Without the flag on the wire there is nothing to decide with, and
  // the reappearing-text bug comes back for cut-over books.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Declaring Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  // Let the body register with the sync engine before judging its writes.
  await typeInEditor(page, "First sentence, to get sync going.");
  await page.waitForTimeout(6000);

  const declared: boolean[] = [];
  page.on("request", (r) => {
    if (r.method() !== "PUT") return;
    if (!/\/api\/books\/[^/]+\/chapters\//.test(r.url())) return;
    const body = r.postData() ?? "";
    if (!body.includes('"content"') || body.includes('"content":null')) return;
    declared.push(body.includes('"sync_owned":true'));
  });

  await typeInEditor(page, " Second sentence, typed while sync is on.");
  await page.waitForTimeout(6000);

  expect(declared.length, "the body write must still be sent").toBeGreaterThan(0);
  expect(
    declared.every(Boolean),
    `every body write must declare sync ownership: ${JSON.stringify(declared)}`,
  ).toBe(true);
});

test("sync off: the chapter body is written without the declaration", async ({ page }) => {
  // The counterpart. Sync is off, so REST is the only writer there is and the write
  // must not claim otherwise.
  await registerNewUser(page);
  await createBook(page, "Rest Writer Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  const bodyWrites: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/chapters\//.test(r.url())) {
      const body = r.postData() ?? "";
      if (!body.includes('"content"')) return;
      expect(body).toContain('"sync_owned":false');
      bodyWrites.push(new URL(r.url()).pathname);
    }
  });

  await typeInEditor(page, "Written with sync off.");
  await page.waitForTimeout(6000);

  expect(bodyWrites.length, "REST is the only writer when sync is off").toBeGreaterThan(0);
});
