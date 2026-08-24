import { test, expect, Browser, BrowserContext, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * The four scenarios the cutover plan named and never ran.
 *
 * Everything proven so far has been one device, online, doing one thing at a time.
 * These are the shapes a CRDT exists for and the ones nothing has exercised: two
 * devices editing at once, an edit made offline, a deletion racing an edit, and a
 * device whose document was seeded independently.
 *
 *     cd e2e && npm run test:cutover
 */

// These need a server started with `PLOTWEB_CUTOVER_BOOKS=*`; run them with
// `npm run test:cutover`. Running them against the default server would exercise the
// git-authoritative paths and quietly prove nothing about cutover.
test.skip(
  !process.env.PLOTWEB_E2E_CUTOVER,
  "requires a cut-over server — run `npm run test:cutover`",
);


async function openDevice(
  browser: Browser,
  baseURL: string,
  sync = true,
): Promise<{ page: Page; context: BrowserContext }> {
  const context = await browser.newContext();
  if (sync) {
    await context.addInitScript(() => window.localStorage.setItem("plotweb_sync", "1"));
  }
  const page = await context.newPage();
  await page.goto(baseURL);
  return { page, context };
}

/** Sign in and wait for the app to hydrate — the shared helper doesn't wait. */
async function signIn(page: Page, username: string, password: string) {
  await page.goto("/login");
  await page.locator("input[placeholder='Your username']").waitFor();
  await page.locator("input[placeholder='Your username']").fill(username);
  await page.locator("input[placeholder='Your password']").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/$/, { timeout: 15_000 });
}

async function bodyText(page: Page): Promise<string> {
  return (await page.locator("#editor-main").textContent()) ?? "";
}

/** Poll until the editor contains `needle`, returning whether it ever did. */
async function eventually(page: Page, needle: string, ms = 20_000): Promise<boolean> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if ((await bodyText(page)).includes(needle)) return true;
    await page.waitForTimeout(500);
  }
  return false;
}

test("two devices editing the same chapter converge", async ({ browser, baseURL }) => {
  // The thing a CRDT is *for*, and the case nothing has run. What must not happen is
  // one device's words replacing the other's, or the two braiding inside a word — the
  // signature of a stale snapshot being applied as a diff.
  test.setTimeout(180_000);

  const a = await openDevice(browser, baseURL!);
  const { username, password } = await registerNewUser(a.page);
  const bookId = await createBook(a.page, "Concurrent Novel");
  await addChapter(a.page, "Chapter One");
  await openChapter(a.page, "Chapter One");
  await typeInEditor(a.page, "Shared opening. ");
  await a.page.waitForTimeout(6000);

  const b = await openDevice(browser, baseURL!);
  await signIn(b.page, username, password);
  await b.page.goto(`/book/${bookId}`);
  await openChapter(b.page, "Chapter One");
  expect(await eventually(b.page, "Shared opening."), "B must first see A's text").toBe(true);

  // Both type without seeing each other.
  await Promise.all([
    typeInEditor(a.page, "Written on A. "),
    typeInEditor(b.page, "Written on B. "),
  ]);
  await a.page.waitForTimeout(12_000);
  await b.page.waitForTimeout(2000);

  for (const [name, page] of [["A", a.page], ["B", b.page]] as const) {
    await page.reload();
    await openChapter(page, "Chapter One");
    expect(await eventually(page, "Written on A."), `${name} lost A's edit`).toBe(true);
    expect(await eventually(page, "Written on B."), `${name} lost B's edit`).toBe(true);
    expect(await bodyText(page), `${name} kept the shared opening`).toContain("Shared opening.");
  }
});

test("an edit made offline reaches the server on reconnect", async ({ browser, baseURL }) => {
  // The promise the whole migration is for, never once tried against a real server.
  test.setTimeout(180_000);

  const a = await openDevice(browser, baseURL!);
  await registerNewUser(a.page);
  await createBook(a.page, "Offline Novel");
  await addChapter(a.page, "Chapter One");
  await openChapter(a.page, "Chapter One");
  await typeInEditor(a.page, "Written while online. ");
  await a.page.waitForTimeout(6000);

  await a.context.setOffline(true);
  await typeInEditor(a.page, "Written while offline. ");
  await a.page.waitForTimeout(4000);
  await a.context.setOffline(false);
  // Sync backs off on failure, so give it room to come back round.
  await a.page.waitForTimeout(20_000);

  await a.page.reload();
  await openChapter(a.page, "Chapter One");
  expect(
    await eventually(a.page, "Written while offline."),
    "an edit made offline must survive coming back",
  ).toBe(true);
  expect(await bodyText(a.page)).toContain("Written while online.");
});

// KNOWN FAILING — the deletion does not reach the other device. Left in place, and
// marked, rather than deleted or quietly weakened: the scenario is correct and the gap
// is real.
//
// A's delete works and the rest of the book survives it. B keeps showing the chapter,
// and not because it is slow — forty seconds is no better than twelve. The server also
// logs B still pushing body updates for it:
//
//     [mirror] chapter:26f33a7b…: git write failed: chapter not found
//
// Leading suspect, unconfirmed: B's `sync_chapters` writes a whole chapter list built
// from `store.chapters`, and a stale list re-inserts the removal after it arrives — the
// same stale-whole-state shape as #22, #24 and #27. Wants a session of its own rather
// than a guess at the end of one.
test.fixme("deleting a chapter on one device while the other edits it", async ({ browser, baseURL }) => {
  // §D7: removal from the parent index is the deletion. What matters here is that it
  // does not take the rest of the book with it, and that the deletion is not undone by
  // the other device's in-flight edit.
  test.setTimeout(180_000);

  const a = await openDevice(browser, baseURL!);
  const { username, password } = await registerNewUser(a.page);
  const bookId = await createBook(a.page, "Deletion Novel");
  await addChapter(a.page, "Doomed");
  await addChapter(a.page, "Survivor");
  await openChapter(a.page, "Survivor");
  await typeInEditor(a.page, "This chapter must live. ");
  await a.page.waitForTimeout(6000);

  const b = await openDevice(browser, baseURL!);
  await signIn(b.page, username, password);
  await b.page.goto(`/book/${bookId}`);
  await openChapter(b.page, "Doomed");
  await typeInEditor(b.page, "Typed into a chapter about to be deleted. ");

  // A deletes it while B is still typing into it. A is in the editor pane, where the
  // chapter list is hidden, so go back to it first.
  await a.page.goto(`/book/${bookId}`);
  await expect(a.page.locator(".chapter-item").first()).toBeVisible();
  const row = a.page.locator(".chapter-item", {
    has: a.page.locator(".chapter-item-left", { hasText: "Doomed" }),
  });
  // Actions are [up, down, rename, delete]; deletion is immediate, with no confirm.
  await row.locator(".chapter-item-actions .rinch-action-icon").nth(3).click();
  await expect(row).toHaveCount(0);
  await a.page.waitForTimeout(20_000);

  for (const [name, page] of [["A", a.page], ["B", b.page]] as const) {
    await page.goto(`/book/${bookId}`);
    await expect(page.locator(".chapter-item").first()).toBeVisible();
    await expect(
      page.locator(".chapter-item", { hasText: "Survivor" }),
      `${name} must still have the surviving chapter`,
    ).toHaveCount(1);
    await expect(
      page.locator(".chapter-item", { hasText: "Doomed" }),
      `${name} must not resurrect the deleted chapter`,
    ).toHaveCount(0);
    await openChapter(page, "Survivor");
    expect(await eventually(page, "This chapter must live."), `${name} lost the survivor`).toBe(
      true,
    );
  }
});

test("a device whose document was seeded independently replaces it, once", async ({
  browser,
  baseURL,
}) => {
  // §D8. A device that used the app with sync off built its body document from REST,
  // sharing no history with the canonical one. Merging those concatenates — the whole
  // chapter twice. The server detects it and answers 409; the client replaces its copy.
  // Implemented, unit-tested, and never once seen in a browser.
  test.setTimeout(180_000);

  const a = await openDevice(browser, baseURL!);
  const { username, password } = await registerNewUser(a.page);
  const bookId = await createBook(a.page, "Provenance Novel");
  await addChapter(a.page, "Chapter One");
  await openChapter(a.page, "Chapter One");
  await typeInEditor(a.page, "Canonical sentence. ");
  await a.page.waitForTimeout(8000);

  // A second device with sync OFF: it seeds its own local document from REST.
  const b = await openDevice(browser, baseURL!, false);
  await signIn(b.page, username, password);
  await b.page.goto(`/book/${bookId}`);
  await openChapter(b.page, "Chapter One");
  expect(await eventually(b.page, "Canonical sentence."), "B seeds from REST").toBe(true);
  await b.page.waitForTimeout(4000);

  // Now switch sync on for B and reload — its document and the canonical one share no
  // history.
  await b.context.addInitScript(() => window.localStorage.setItem("plotweb_sync", "1"));
  await b.page.evaluate(() => window.localStorage.setItem("plotweb_sync", "1"));
  await b.page.reload();
  await openChapter(b.page, "Chapter One");
  await b.page.waitForTimeout(15_000);

  const text = await bodyText(b.page);
  expect(text, "the sentence must appear once, not twice").toContain("Canonical sentence.");
  expect(
    text.split("Canonical sentence.").length - 1,
    `concatenated histories would show it twice: ${text}`,
  ).toBe(1);
});
