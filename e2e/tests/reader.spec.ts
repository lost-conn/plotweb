import { test, expect, Page } from "@playwright/test";
import { createBetaLink, createBook, registerNewUser, seedLongChapter } from "./helpers";

/**
 * Reader-mode coverage: the paginated beta reader (`/read/{token}`) and the
 * author preview (`/preview/{bookId}`).
 *
 * The reader lays content out in CSS multi-column "pages" (no scroll). The page
 * bar (`.reader-pagebar`) has a prev ActionIcon, an indicator
 * (`.reader-pagebar-indicator`, literal text `"{page+1} / {total}"`) and a next
 * ActionIcon. Pagination is measured asynchronously (rAF + a late second pass),
 * and progress saves are debounced — so every assertion here waits on the
 * indicator text / bookmark items rather than on a fixed timeout.
 *
 * All setup seeds a deliberately long chapter (see `seedLongChapter`) so the
 * chapter spans many pages; each test asserts the total is > 1 before paging.
 */

const READER_ORIGIN = "http://localhost:3000";
const CHAPTER = "The Long Chapter";

/** Parse the `"{page} / {total}"` indicator into 1-based numbers. */
async function readIndicator(page: Page): Promise<{ page: number; total: number }> {
  const text = (await page.locator(".reader-pagebar-indicator").textContent()) ?? "";
  const m = text.match(/(\d+)\s*\/\s*(\d+)/);
  if (!m) throw new Error(`unparseable page indicator: "${text}"`);
  return { page: Number(m[1]), total: Number(m[2]) };
}

/**
 * Open a chapter in the reader sidebar and wait until it has paginated into more
 * than one page. Returns the measured total. Fails loudly (rather than silently
 * passing on a 1-page chapter) if the seeded content wasn't long enough.
 */
async function openChapterAndAwaitMultiPage(page: Page, chapterTitle: string): Promise<number> {
  await page.locator(".reader-chapter-item", { hasText: chapterTitle }).click();
  await expect(page.locator("#reader-content")).toContainText("Paragraph 1.");
  // Pagination is async: poll the indicator until the total settles above 1.
  await expect
    .poll(async () => (await readIndicator(page)).total, { timeout: 10_000 })
    .toBeGreaterThan(1);
  const { total } = await readIndicator(page);
  return total;
}

/**
 * Author-side setup shared by every test: register, make a book with one long
 * chapter, and (for beta tests) mint a beta link. Returns the ids/token.
 */
async function seedBook(page: Page, reader = "Reader"): Promise<{ bookId: string; token: string }> {
  await registerNewUser(page);
  const bookId = await createBook(page, "Reader Mode Novel");
  await seedLongChapter(page, bookId, CHAPTER);
  const token = await createBetaLink(page, bookId, reader);
  return { bookId, token };
}

test.describe("beta reader pagination", () => {
  test("paginates with the prev/next buttons and clamps at the first page", async ({
    page,
    browser,
  }) => {
    const { token } = await seedBook(page, "Paginator");

    const ctx = await browser.newContext({ baseURL: READER_ORIGIN });
    const reader = await ctx.newPage();
    try {
      await reader.goto(`/read/${token}`);
      const total = await openChapterAndAwaitMultiPage(reader, CHAPTER);
      expect(total).toBeGreaterThan(1);

      const prev = reader.locator(".reader-pagebar .rinch-action-icon").nth(0);
      const next = reader.locator(".reader-pagebar .rinch-action-icon").nth(1);

      // Starts on page 1.
      expect(await readIndicator(reader)).toEqual({ page: 1, total });

      // Next → page 2.
      await next.click();
      await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`2 / ${total}`);

      // Prev → back to page 1.
      await prev.click();
      await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`1 / ${total}`);

      // Prev at page 1 clamps: stays on page 1.
      await prev.click();
      await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`1 / ${total}`);
    } finally {
      await ctx.close();
    }
  });

  test("paginates with the arrow keys", async ({ page, browser }) => {
    const { token } = await seedBook(page, "Keyboarder");

    const ctx = await browser.newContext({ baseURL: READER_ORIGIN });
    const reader = await ctx.newPage();
    try {
      await reader.goto(`/read/${token}`);
      const total = await openChapterAndAwaitMultiPage(reader, CHAPTER);

      // The keydown handler is document-level; ensure focus isn't in a control
      // by clicking the (non-focusable) topbar first.
      await reader.locator(".reader-topbar").click();

      await reader.keyboard.press("ArrowRight");
      await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`2 / ${total}`);

      await reader.keyboard.press("ArrowRight");
      await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`3 / ${total}`);

      await reader.keyboard.press("ArrowLeft");
      await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`2 / ${total}`);
    } finally {
      await ctx.close();
    }
  });
});

test("bookmark: add at a page, persists, and jumps back to it", async ({ page, browser }) => {
  const { token } = await seedBook(page, "Bookmarker");

  const ctx = await browser.newContext({ baseURL: READER_ORIGIN });
  const reader = await ctx.newPage();
  try {
    await reader.goto(`/read/${token}`);
    const total = await openChapterAndAwaitMultiPage(reader, CHAPTER);

    const next = reader.locator(".reader-pagebar .rinch-action-icon").nth(1);
    // Move to page 2 (index 1) and bookmark there.
    await next.click();
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`2 / ${total}`);

    // The Bookmark ActionIcon is the first action in the desktop topbar.
    await reader.locator(".reader-topbar .rinch-action-icon").nth(0).click();

    // A bookmark item appears in the sidebar, labelled with the page.
    const bookmark = reader.locator(".reader-bookmark-item");
    await expect(bookmark).toHaveCount(1);
    await expect(bookmark.locator(".reader-bookmark-label")).toContainText("p.2");

    // Persisted server-side (not just an optimistic UI insert).
    await expect
      .poll(async () => {
        const resp = await reader.request.get(`/api/beta/${token}/bookmarks`);
        const list = (await resp.json()) as Array<{ page: number }>;
        return list.length === 1 && list[0].page === 1;
      })
      .toBe(true);

    // Go back to page 1, then click the bookmark → jumps to the bookmarked page.
    await reader.locator(".reader-pagebar .rinch-action-icon").nth(0).click();
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`1 / ${total}`);

    await bookmark.locator(".reader-bookmark-label").click();
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`2 / ${total}`);
  } finally {
    await ctx.close();
  }
});

test("resume position: reloading returns to the last-read page", async ({ page, browser }) => {
  const { token } = await seedBook(page, "Resumer");

  const ctx = await browser.newContext({ baseURL: READER_ORIGIN });
  const reader = await ctx.newPage();
  try {
    await reader.goto(`/read/${token}`);
    const total = await openChapterAndAwaitMultiPage(reader, CHAPTER);
    expect(total).toBeGreaterThan(2);

    const next = reader.locator(".reader-pagebar .rinch-action-icon").nth(1);
    // Advance to page 3 (index 2).
    await next.click();
    await next.click();
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`3 / ${total}`);

    // The progress PUT is debounced (500ms): poll the beta view until it lands.
    await expect
      .poll(async () => {
        const resp = await reader.request.get(`/api/beta/${token}`);
        const view = (await resp.json()) as { last_page: number };
        return view.last_page;
      })
      .toBe(2);

    // Reload: the reader resumes at the saved chapter + page.
    await reader.goto(`/read/${token}`);
    await expect(reader.locator("#reader-content")).toContainText("Paragraph 1.");
    await expect
      .poll(async () => (await readIndicator(reader)).page, { timeout: 10_000 })
      .toBe(3);
    expect((await readIndicator(reader)).total).toBe(total);
  } finally {
    await ctx.close();
  }
});

test("author preview renders and paginates (no persistence)", async ({ page }) => {
  // Author stays logged in; preview reads the authenticated book endpoints.
  await registerNewUser(page);
  const bookId = await createBook(page, "Preview Novel");
  await seedLongChapter(page, bookId, CHAPTER);

  await page.goto(`/preview/${bookId}`);
  // Preview badge confirms we're in author-preview mode.
  await expect(page.getByText("Preview", { exact: true }).first()).toBeVisible();

  const total = await openChapterAndAwaitMultiPage(page, CHAPTER);
  expect(total).toBeGreaterThan(1);
  expect(await readIndicator(page)).toEqual({ page: 1, total });

  // Paginates: next advances the indicator.
  await page.locator(".reader-pagebar .rinch-action-icon").nth(1).click();
  await expect(page.locator(".reader-pagebar-indicator")).toHaveText(`2 / ${total}`);

  // Author preview intentionally has no bookmark control.
  await expect(page.locator(".reader-bookmark-item")).toHaveCount(0);
});

test("swipe (touch) turns a page", async ({ page, browser }) => {
  const { token } = await seedBook(page, "Swiper");

  // A touch-enabled context so the reader's touchstart/touchend swipe path is live.
  const ctx = await browser.newContext({ baseURL: READER_ORIGIN, hasTouch: true });
  const reader = await ctx.newPage();
  try {
    await reader.goto(`/read/${token}`);
    const total = await openChapterAndAwaitMultiPage(reader, CHAPTER);
    expect(await readIndicator(reader)).toEqual({ page: 1, total });

    // Dispatch a genuine horizontal left-swipe over the reading viewport. The
    // reader's document-level touchend handler reads changedTouches[0].clientX
    // and turns the page when the drag exceeds 45px with an empty selection.
    const swipe = async (dx: number) => {
      await reader.evaluate((dx) => {
        const el = document.querySelector("#reader-viewport") as HTMLElement;
        const rect = el.getBoundingClientRect();
        const y = rect.top + rect.height / 2;
        const startX = rect.left + rect.width / 2;
        const mk = (x: number) =>
          new Touch({ identifier: 1, target: el, clientX: x, clientY: y });
        el.dispatchEvent(
          new TouchEvent("touchstart", {
            bubbles: true,
            cancelable: true,
            touches: [mk(startX)],
            targetTouches: [mk(startX)],
            changedTouches: [mk(startX)],
          }),
        );
        el.dispatchEvent(
          new TouchEvent("touchend", {
            bubbles: true,
            cancelable: true,
            touches: [],
            targetTouches: [],
            changedTouches: [mk(startX + dx)],
          }),
        );
      }, dx);
    };

    // Swipe left → next page.
    await swipe(-120);
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`2 / ${total}`);

    // Swipe right → previous page.
    await swipe(120);
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`1 / ${total}`);
  } finally {
    await ctx.close();
  }
});
