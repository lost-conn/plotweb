import { test, expect, Page } from "@playwright/test";
import { registerNewUser, createBook, openSettings } from "./helpers";

/**
 * Agent access (personal access tokens), end to end through the Settings page:
 * mint a token scoped to one book, see the raw value exactly once, use it as a
 * bearer credential, revoke it, and watch it stop working.
 */

/** `GET /api/tokens/whoami` from inside the page, with only the bearer token. */
async function whoami(page: Page, token: string) {
  return page.evaluate(async (t) => {
    const r = await fetch("/api/tokens/whoami", {
      headers: { Authorization: "Bearer " + t },
      credentials: "omit",
    });
    return { status: r.status, body: await r.json().catch(() => null) };
  }, token);
}

test("create a scoped token, use it once, revoke it", async ({ page }) => {
  await registerNewUser(page);
  const alphaId = await createBook(page, "Alpha manuscript");
  await page.goto("/");
  // With a book on the shelf, "New Book" becomes the shelf's "New book" tile.
  await page.locator(".bk-new").click();
  await page.locator("input[placeholder='Book title']").fill("Beta manuscript");
  await page.locator(".rinch-modal__body:visible").getByRole("button", { name: "Create" }).click();
  await expect(page.locator(".bk-meta-title", { hasText: "Beta manuscript" })).toBeVisible();

  await openSettings(page);
  await expect(page.getByText("No tokens yet.")).toBeVisible();

  // Scope it to Alpha only: switch "All books" off and tick one book.
  await page.locator("input[placeholder='e.g. Claude on my laptop']").fill("Claude on my laptop");
  await page.locator(".token-form .rinch-switch__input").click();
  await expect(page.locator(".token-form .rinch-switch")).not.toHaveClass(/rinch-switch--checked/);
  await expect(page.locator(".token-books")).toBeVisible();
  await page.locator(".token-books").getByText("Alpha manuscript", { exact: true }).click();
  await page.getByRole("button", { name: "Create token" }).click();

  // The raw token is shown once, with the warning.
  const reveal = page.locator(".token-reveal");
  await expect(reveal).toBeVisible();
  await expect(reveal).toContainText("You won't see it again");
  const token = (await reveal.locator(".token-reveal-value").textContent())!.trim();
  expect(token).toMatch(/^pw_[A-Za-z0-9_-]{43}$/);

  await reveal.getByRole("button", { name: "Done" }).click();
  await expect(reveal).toHaveCount(0);

  // Listed by label and prefix, scoped to Alpha — and the raw token is gone.
  const row = page.locator(".token-row", { hasText: "Claude on my laptop" });
  await expect(row).toBeVisible();
  await expect(row.locator(".token-prefix")).toHaveText(`pw_${token.slice(3, 11)}…`);
  await expect(row.locator(".token-meta")).toContainText("Alpha manuscript");
  await expect(row.locator(".token-meta")).not.toContainText("Beta manuscript");
  expect(await page.locator("body").innerText()).not.toContain(token);

  // The token works as a bearer credential, with the right label and scope.
  const ok = await whoami(page, token);
  expect(ok.status).toBe(200);
  expect(ok.body.token_label).toBe("Claude on my laptop");
  expect(ok.body.book_ids).toEqual([alphaId]);

  // A reload of Settings never shows the raw token again.
  await page.reload();
  await expect(page.locator(".token-row", { hasText: "Claude on my laptop" })).toBeVisible();
  expect(await page.locator("body").innerText()).not.toContain(token);

  // Revoke behind a confirmation.
  await page
    .locator(".token-row", { hasText: "Claude on my laptop" })
    .getByRole("button", { name: "Revoke" })
    .click();
  const dialog = page.locator(".rinch-modal__body:visible");
  await expect(dialog).toContainText("loses access");
  await dialog.getByRole("button", { name: "Revoke" }).click();
  await expect(page.locator(".token-row")).toHaveCount(0);
  await expect(page.getByText("No tokens yet.")).toBeVisible();

  const gone = await whoami(page, token);
  expect(gone.status).toBe(401);
});

test("settings leads back to the library", async ({ page }) => {
  await registerNewUser(page);
  await openSettings(page);
  await page.getByRole("button", { name: "Library" }).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.locator(".dash-topbar")).toBeVisible();
});
