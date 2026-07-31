import { test, expect, Browser, Page } from "@playwright/test";
import { addChapter, createBook, login, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * Chapter **body** sync between two devices (sync engine slice 4).
 *
 * Two browser contexts are two devices: separate cookie jars and, crucially,
 * separate IndexedDB, so each keeps its own local Automerge documents. They talk
 * to one server, as real devices would.
 *
 * Sync is off unless switched on, so each context sets the flag before the app
 * boots. Body sync also runs only while a body is open (background sweeps of
 * unopened bodies are slice 5), so both devices sit on the same chapter.
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

test("body sync: a chapter edit on one device reaches another", async ({ browser, baseURL }) => {
  test.setTimeout(120_000);

  // Device A: account, book, chapter, prose.
  const deviceA = await openDevice(browser, baseURL!);
  const { username, password } = await registerNewUser(deviceA);
  await createBook(deviceA, "Sync Novel");
  await addChapter(deviceA, "Chapter One");
  await openChapter(deviceA, "Chapter One");
  await typeInEditor(deviceA, "Written on device A.");
  // Let the REST autosave settle and the body's first sync exchange run.
  await deviceA.waitForTimeout(4000);

  // Device B: same account, same chapter, its own local store.
  const deviceB = await openDevice(browser, baseURL!);
  await login(deviceB, username, password);
  await deviceB.getByText("Sync Novel", { exact: true }).first().click();
  await expect(deviceB).toHaveURL(/\/book\/[0-9a-f-]{36}/);
  await openChapter(deviceB, "Chapter One");
  await expect(deviceB.locator("#editor-main")).toContainText("Written on device A.");

  // The decisive part: B edits, and A — sitting on the same chapter, untouched —
  // must converge on its own, with both edits present and neither duplicated.
  await typeInEditor(deviceB, " Added on device B.");
  await deviceB.waitForTimeout(4000);

  await expect(deviceA.locator("#editor-main")).toContainText("Added on device B.", {
    timeout: 60_000,
  });
  const shownOnA = await deviceA.locator("#editor-main").innerText();
  expect(shownOnA).toContain("Written on device A.");
  expect(
    shownOnA.match(/Written on device A\./g)?.length,
    `chapter text must not be duplicated by a disjoint-history merge: ${shownOnA}`,
  ).toBe(1);

  await deviceA.context().close();
  await deviceB.context().close();
});
