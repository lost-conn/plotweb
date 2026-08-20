import { test, expect, Browser, Page } from "@playwright/test";
import { addChapter, createBook, registerNewUser } from "./helpers";

/**
 * A second device must learn about work done on the first — *without* sync.
 *
 * The local-first layer keeps a per-account `user:` document (the dashboard's book
 * list) and a per-book `book:` document (chapters, notes, the tree). Both were entered
 * the same way: if a local document already existed, load it and discard the REST
 * payload, which was only ever used to seed a document that wasn't there yet.
 *
 * That makes "local wins" absolute rather than protective. It was meant to stop a stale
 * REST fetch clobbering an edit made offline; what it also did was throw away every
 * addition made anywhere else. A device that had loaded the app once never saw another
 * device's new book again — reported from production exactly that way — and never saw a
 * new chapter in a book it had already opened.
 *
 * Sync would carry these, but sync is off by default, so this is the shipped behaviour
 * for anyone signing in on a phone as well as a laptop.
 */

/** Sign in and wait for the app to actually come up — the WASM bundle hydrates after
 * navigation, so the shared `login` helper's fire-and-forget click is not enough here. */
async function signIn(page: Page, username: string, password: string) {
  await page.goto("/login");
  await page.locator("input[placeholder='Your username']").waitFor();
  await page.locator("input[placeholder='Your username']").fill(username);
  await page.locator("input[placeholder='Your password']").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/$/, { timeout: 15_000 });
}

/** Create a book from a dashboard that already has one.
 *
 * The shared `createBook` helper clicks a `button` labelled "New Book", which only
 * exists in the empty state; once there are books the control is a `.book-card-new`
 * tile instead. */
async function createSecondBook(page: Page, title: string) {
  await page.locator(".book-card-new").click();
  await page.locator("input[placeholder='Book title']").fill(title);
  await page.locator(".rinch-modal__body:visible").getByRole("button", { name: "Create" }).click();
  await expect(page.getByText(title, { exact: true }).first()).toBeVisible();
}

/** A device: its own context, so its own IndexedDB. Sync deliberately left off. */
async function openDevice(browser: Browser, baseURL: string): Promise<Page> {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(baseURL);
  return page;
}

test("a book added on one device appears on a device that loaded earlier", async ({
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);

  // Device A signs up and loads the dashboard, which seeds its local `user:` doc.
  const deviceA = await openDevice(browser, baseURL!);
  const { username, password } = await registerNewUser(deviceA);
  await createBook(deviceA, "First Novel");
  await deviceA.goto(baseURL!);
  await expect(deviceA.getByText("First Novel")).toBeVisible();

  // Device B, same account, adds a book A has never heard of.
  const deviceB = await openDevice(browser, baseURL!);
  await signIn(deviceB, username, password);
  await expect(deviceB.getByText("First Novel")).toBeVisible();
  await createSecondBook(deviceB, "Written Elsewhere");

  // A reloads. The server knows about the new book; A must too.
  await deviceA.goto(baseURL!);
  await expect(
    deviceA.getByText("Written Elsewhere"),
    "a book created on another device must reach a device that had already loaded",
  ).toBeVisible({ timeout: 15_000 });
  // And A's own book is still there — learning must not mean replacing.
  await expect(deviceA.getByText("First Novel")).toBeVisible();
});

test("a chapter added on one device appears in a book the other had already opened", async ({
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);

  const deviceA = await openDevice(browser, baseURL!);
  const { username, password } = await registerNewUser(deviceA);
  const bookId = await createBook(deviceA, "Shared Novel");
  await addChapter(deviceA, "Chapter One");
  // Opening the book seeds A's local `book:` doc.
  await expect(deviceA.locator(".chapter-item").first()).toBeVisible();

  const deviceB = await openDevice(browser, baseURL!);
  await signIn(deviceB, username, password);
  await deviceB.goto(`/book/${bookId}`);
  await expect(deviceB.getByRole("button", { name: "Add Chapter" }).first()).toBeVisible();
  await addChapter(deviceB, "Chapter Two");

  await deviceA.goto(`/book/${bookId}`);
  await expect(
    deviceA.locator(".chapter-item", { hasText: "Chapter Two" }),
    "a chapter created on another device must reach a device that had already opened the book",
  ).toBeVisible({ timeout: 15_000 });
  await expect(deviceA.locator(".chapter-item", { hasText: "Chapter One" })).toBeVisible();
});
