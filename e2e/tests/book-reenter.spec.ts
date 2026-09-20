import { test, expect, Page } from "@playwright/test";
import { createBook, registerNewUser } from "./helpers";

/**
 * Leaving a book and entering one again must render, not blank the page.
 *
 * The Google Fonts catalog lives in a `thread_local!` signal (`fonts.rs`), and a
 * thread-local initializes on first touch. That first touch was
 * `fetch_font_catalog()` inside the book page's own render, so rinch attributed
 * the "global" signal to that page's scope: leaving the book disposed the scope
 * and freed the signal, the thread-local handle lived on, and the next book
 * page read the dead slot — a `Signal::get() on a freed signal` panic that
 * unwound the whole render and left a blank body. The signal is now created
 * `unowned` (app lifetime). This drives the exact sequence and listens for the
 * panic: a wasm panic surfaces as a `pageerror`, which a DOM assertion alone
 * could miss if the previous render's nodes were still on screen.
 */

/** The dashboard shelf's "open book" affordance: the jacket carrying the title. */
const jacket = (page: Page, title: string) =>
  page.locator(".shelf .bk", { has: page.locator(".bk-meta-title", { hasText: title }) }).first();

/** Leave via the workspace's "All books" tool and wait for the shelf. */
async function backToDashboard(page: Page) {
  await page.locator('.ws-tools .tool[data-tip="All books"]').click();
  await expect(page.locator(".shelf .bk").first()).toBeVisible();
}

async function expectBookOpen(page: Page, title: string) {
  await expect(page).toHaveURL(/\/book\/[0-9a-f-]{36}/);
  await expect(page.locator(".book-workspace")).toBeVisible();
  await expect(page.locator(".ws-book-title", { hasText: title })).toBeVisible();
}

test("a book opens again after going back to the dashboard", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (err) => pageErrors.push(String(err)));

  await registerNewUser(page);
  await createBook(page, "Reenter Novel");
  await expectBookOpen(page, "Reenter Novel");

  await backToDashboard(page);
  await jacket(page, "Reenter Novel").click();
  await expectBookOpen(page, "Reenter Novel");

  // And twice more: the catalog fetch runs on every book page, so every
  // re-entry after the first is a read of the same global.
  for (let i = 0; i < 2; i++) {
    await backToDashboard(page);
    await jacket(page, "Reenter Novel").click();
    await expectBookOpen(page, "Reenter Novel");
  }

  expect(pageErrors, pageErrors.join("\n")).toEqual([]);
});
