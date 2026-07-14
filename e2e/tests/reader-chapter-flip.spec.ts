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

async function readIndicator(page: Page): Promise<{ page: number; total: number }> {
  const text = (await page.locator(".reader-pagebar-indicator").textContent()) ?? "";
  const m = text.match(/(\d+)\s*\/\s*(\d+)/);
  if (!m) throw new Error(`unparseable indicator: "${text}"`);
  return { page: Number(m[1]), total: Number(m[2]) };
}

const nextBtn = (p: Page) => p.locator(".reader-pagebar .rinch-action-icon").nth(1);
const prevBtn = (p: Page) => p.locator(".reader-pagebar .rinch-action-icon").nth(0);

/** Open a chapter from the sidebar and wait until it paginates past one page. */
async function openMultiPage(p: Page, title: string): Promise<number> {
  await p.locator(".reader-chapter-item", { hasText: title }).click();
  await expect(p.locator("#reader-content")).toContainText("Paragraph 1.");
  await expect.poll(async () => (await readIndicator(p)).total, { timeout: 10_000 }).toBeGreaterThan(1);
  return (await readIndicator(p)).total;
}

/** Click Next until on the last page. */
async function goToLastPage(p: Page, total: number) {
  for (let i = (await readIndicator(p)).page; i < total; i++) {
    await nextBtn(p).click();
    await expect(p.locator(".reader-pagebar-indicator")).toHaveText(`${i + 1} / ${total}`);
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

    // Prev on page 1 of the FIRST chapter is a no-op (no wrap-around).
    await prevBtn(reader).click();
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`1 / ${total1}`);
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

    // Next on the LAST page of the LAST chapter is a no-op.
    await openMultiPage(reader, "Chapter Two");
    const t2 = (await readIndicator(reader)).total;
    await goToLastPage(reader, t2);
    await nextBtn(reader).click();
    await expect(reader.locator(".reader-topbar")).toContainText("Chapter Two");
    await expect(reader.locator(".reader-pagebar-indicator")).toHaveText(`${t2} / ${t2}`);
  } finally {
    await ctx.close();
  }
});
