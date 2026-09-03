import { test, expect, Browser, BrowserContext, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * Who decides whether sync runs — with nobody asking for it.
 *
 * Sync used to be one global opt-in flag, off by default, and every other sync spec
 * sets it. That left the configuration real authors actually run — the flag untouched —
 * untested in both directions, and under one writer the two directions are very
 * different:
 *
 * - a **cut-over** book takes body edits through sync and nothing else, so sync off
 *   means writing that never leaves the device;
 * - a **git-backed** book is still written over REST, and sync on would make two live
 *   writers of the same body with nothing mirroring between them.
 *
 * So: cutover implies sync, nothing else does. One of these two runs per invocation —
 * the cut-over half needs `npm run test:cutover`, the git-backed half the default
 * server.
 */

/** A device nobody has configured: no `plotweb_sync` in local storage. */
async function openPlainDevice(
  browser: Browser,
  baseURL: string,
): Promise<{ page: Page; context: BrowserContext; syncCalls: string[] }> {
  const context = await browser.newContext();
  const page = await context.newPage();
  const syncCalls: string[] = [];
  page.on("request", (r) => {
    const url = new URL(r.url()).pathname;
    if (url.includes("/sync/") || url === "/api/sync/user") syncCalls.push(url);
  });
  await page.goto(baseURL);
  return { page, context, syncCalls };
}

async function bodyText(page: Page): Promise<string> {
  return (await page.locator("#editor-main").textContent()) ?? "";
}

async function eventually(page: Page, needle: string, ms = 20_000): Promise<boolean> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if ((await bodyText(page)).includes(needle)) return true;
    await page.waitForTimeout(500);
  }
  return false;
}

test.describe("a cut-over book", () => {
  test.skip(
    !process.env.PLOTWEB_E2E_CUTOVER,
    "requires a cut-over server — run `npm run test:cutover`",
  );

  test("syncs without being asked, and says the save is not local-only", async ({
    browser,
    baseURL,
  }) => {
    // The regression this replaced a global flag to prevent: on a cut-over book the
    // client sends no body content over REST and the server drops any that arrives, so
    // a device where sync never started is writing to itself while the header says
    // "Saved". Nothing here sets `plotweb_sync`.
    test.setTimeout(180_000);

    const a = await openPlainDevice(browser, baseURL!);
    const { username, password } = await registerNewUser(a.page);
    const bookId = await createBook(a.page, "Unasked Sync Novel");
    await addChapter(a.page, "Chapter One");
    await openChapter(a.page, "Chapter One");
    await typeInEditor(a.page, "Carried by sync alone. ");
    await a.page.waitForTimeout(8000);

    expect(
      a.syncCalls.length,
      "a cut-over book must reach the sync endpoints unasked",
    ).toBeGreaterThan(0);

    // The author-facing half: no kill-switch banner, and the indicator must not be
    // hedging about where the save went.
    await expect(a.page.getByText("stays on this device")).toHaveCount(0);
    await expect(a.page.getByText("Saved on this device")).toHaveCount(0);

    // The proof that it left: a second device, also unconfigured, signing into the
    // same account. Only sync could have carried the text — the REST save didn't.
    const b = await openPlainDevice(browser, baseURL!);
    await b.page.goto("/login");
    await b.page.locator("input[placeholder='Your username']").waitFor();
    await b.page.locator("input[placeholder='Your username']").fill(username);
    await b.page.locator("input[placeholder='Your password']").fill(password);
    await b.page.getByRole("button", { name: "Sign in" }).click();
    await expect(b.page).toHaveURL(/\/$/, { timeout: 15_000 });
    await b.page.goto(`/book/${bookId}`);
    await openChapter(b.page, "Chapter One");
    expect(
      await eventually(b.page, "Carried by sync alone."),
      "a second unconfigured device must see writing the first one only sent through sync",
    ).toBe(true);
  });
});

test.describe("a git-backed book", () => {
  test.skip(
    !!process.env.PLOTWEB_E2E_CUTOVER,
    "the default server is the git-authoritative one",
  );

  test("does not sync when nothing asked it to", async ({ browser, baseURL }) => {
    // The other side of the gate. Before cutover the REST write is the only writer,
    // and the canonical copy is a mirror the server maintains; a client syncing this
    // book would push ops into that copy while its own PUTs rewrite git, and nothing
    // carries either way. Turning sync on by default is what would have done it.
    test.setTimeout(120_000);

    const a = await openPlainDevice(browser, baseURL!);
    await registerNewUser(a.page);
    await createBook(a.page, "Git Backed Novel");
    await addChapter(a.page, "Chapter One");
    await openChapter(a.page, "Chapter One");
    await typeInEditor(a.page, "Written the old way. ");
    await a.page.waitForTimeout(8000);

    expect(
      a.syncCalls,
      "no book here is cut over, so nothing should have touched a sync endpoint",
    ).toEqual([]);

    // And the REST path still carries it: a reload reads the server's copy back.
    await a.page.reload();
    await openChapter(a.page, "Chapter One");
    expect(await eventually(a.page, "Written the old way."), "the REST save must stand").toBe(
      true,
    );
  });
});
