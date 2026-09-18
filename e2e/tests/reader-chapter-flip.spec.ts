import { test, expect, Page } from "@playwright/test";
import {
  createBetaLink,
  createBook,
  registerNewUser,
  seedLongChapter,
} from "./helpers";

/**
 * Paging off either end of a chapter flips to the adjacent chapter: Next on the
 * last page opens the next chapter at page 1; Prev on page 1 opens the previous
 * chapter at its LAST page (natural reading flow). At the very ends it no-ops.
 */

const READER_ORIGIN = "http://localhost:3000";

/** Parse the "{page} / {total}" pair out of the folio row's text. */
async function readIndicator(page: Page): Promise<{ page: number; total: number }> {
  const text = (await page.locator(".reader-folio-row").textContent()) ?? "";
  const m = text.match(/(\d+)\s*\/\s*(\d+)/);
  if (!m) throw new Error(`unparseable folio indicator: "${text}"`);
  return { page: Number(m[1]), total: Number(m[2]) };
}

// Page turns live in the margins now, not a page bar.
const nextBtn = (p: Page) => p.locator(".reader-turn.r");
const prevBtn = (p: Page) => p.locator(".reader-turn.l");

/** Open a chapter from Contents and wait until it paginates past one page. */
async function openMultiPage(p: Page, title: string): Promise<number> {
  // Contents is opened on demand now — the toggle is the first action-icon
  // in the desktop topbar.
  await p.locator(".reader-topbar .rinch-action-icon").first().click();
  await p.locator(".reader-chapter-item", { hasText: title }).click();
  await expect(p.locator("#reader-content")).toContainText("Paragraph 1.");
  await expect.poll(async () => (await readIndicator(p)).total, { timeout: 10_000 }).toBeGreaterThan(1);
  return (await readIndicator(p)).total;
}

/** Click Next until on the last page. */
async function goToLastPage(p: Page, total: number) {
  for (let i = (await readIndicator(p)).page; i < total; i++) {
    await nextBtn(p).click();
    await expect.poll(async () => (await readIndicator(p)).page).toBe(i + 1);
  }
}

test("reader: Next/Prev flip between chapters at the page boundaries", async ({ page, browser }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Chapter Flip Novel");
  // Two distinct, deliberately short-ish multi-page chapters.
  await seedLongChapter(page, bookId, "Chapter One", 24);
  await seedLongChapter(page, bookId, "Chapter Two", 24);
  const token = await createBetaLink(page, bookId, "Flipper");

  const ctx = await browser.newContext({ baseURL: READER_ORIGIN });
  const reader = await ctx.newPage();
  try {
    await reader.goto(`/read/${token}`);

    const total1 = await openMultiPage(reader, "Chapter One");
    await expect(reader.locator(".reader-topbar")).toContainText("Chapter One");

    // Prev on page 1 of the FIRST chapter is a no-op (no wrap-around) — the
    // left turn reads as disabled, since there's no previous chapter either.
    await expect(prevBtn(reader)).toHaveClass(/disabled/);
    await prevBtn(reader).click({ force: true });
    expect((await readIndicator(reader)).page).toBe(1);
    await expect(reader.locator(".reader-topbar")).toContainText("Chapter One");

    // Go to the last page of Chapter One, then Next → Chapter Two, page 1.
    await goToLastPage(reader, total1);
    await nextBtn(reader).click();
    await expect(reader.locator(".reader-topbar")).toContainText("Chapter Two");
    await expect.poll(async () => (await readIndicator(reader)).total, { timeout: 10_000 }).toBeGreaterThan(1);
    const total2 = (await readIndicator(reader)).total;
    expect(await readIndicator(reader)).toEqual({ page: 1, total: total2 });

    // Prev on page 1 of Chapter Two → Chapter One at its LAST page.
    await prevBtn(reader).click();
    await expect(reader.locator(".reader-topbar")).toContainText("Chapter One");
    await expect
      .poll(async () => (await readIndicator(reader)).page, { timeout: 10_000 })
      .toBe(total1);
    expect(await readIndicator(reader)).toEqual({ page: total1, total: total1 });

    // Next on the LAST page of the LAST chapter is a no-op — the right turn
    // reads as disabled, since there's no next chapter either.
    await openMultiPage(reader, "Chapter Two");
    const t2 = (await readIndicator(reader)).total;
    await goToLastPage(reader, t2);
    await expect(nextBtn(reader)).toHaveClass(/disabled/);
    await nextBtn(reader).click({ force: true });
    await expect(reader.locator(".reader-topbar")).toContainText("Chapter Two");
    expect(await readIndicator(reader)).toEqual({ page: t2, total: t2 });
  } finally {
    await ctx.close();
  }
});
