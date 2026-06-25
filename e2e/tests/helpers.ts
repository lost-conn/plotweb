import { Page, expect } from "@playwright/test";

/** A unique-ish username so tests don't collide on the shared server DB. */
export function uniqueUser(prefix = "e2e"): string {
  const rand = Math.random().toString(36).slice(2, 8);
  return `${prefix}_${Date.now().toString(36)}_${rand}`;
}

/** Register a brand-new account through the UI and land on the dashboard. */
export async function registerNewUser(
  page: Page,
  username = uniqueUser(),
  password = "password123",
): Promise<{ username: string; password: string }> {
  // Guarantee a clean, unauthenticated state — otherwise /register redirects
  // straight to the dashboard for an already-logged-in session.
  await page.context().clearCookies();
  await page.goto("/register");
  await page.locator("input[placeholder='Choose a username']").waitFor();
  await page.locator("input[placeholder='Choose a username']").fill(username);
  await page.locator("input[placeholder='your@email.com']").fill(`${username}@example.com`);
  await page.locator("input[placeholder='Choose a password']").fill(password);
  await page.locator("input[placeholder='Repeat your password']").fill(password);
  await page.getByRole("button", { name: "Create account" }).click();

  // Landed on the dashboard (root), which shows the username.
  await expect(page).toHaveURL(/\/$|\/$/);
  await expect(page.getByText(username, { exact: false }).first()).toBeVisible();
  return { username, password };
}

/** Log out via the dashboard topbar icon and land back on /login. */
export async function logout(page: Page) {
  await page.goto("/");
  // The second action-icon in the topbar is the logout control (the first is
  // the dark-mode toggle).
  await page.locator(".dash-topbar-right .rinch-action-icon").nth(1).click();
  await expect(page).toHaveURL(/\/login/);
}

/** Log in through the UI with existing credentials. */
export async function login(page: Page, username: string, password: string) {
  await page.goto("/login");
  await page.locator("input[placeholder='Your username']").fill(username);
  await page.locator("input[placeholder='Your password']").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
}

/** Create a book from the dashboard and return its id (from the URL once open). */
export async function createBook(page: Page, title: string): Promise<string> {
  await page.getByRole("button", { name: "New Book" }).first().click();
  await page.locator("input[placeholder='Book title']").fill(title);
  // The visible "Create" button inside the new-book modal.
  await page.locator(".rinch-modal__body:visible").getByRole("button", { name: "Create" }).click();

  // Open the freshly created book card.
  await page.getByText(title, { exact: true }).first().click();
  await expect(page).toHaveURL(/\/book\/[0-9a-f-]{36}/);
  const url = page.url();
  return url.split("/book/")[1];
}

/** Add a chapter via the "Add Chapter" modal. */
export async function addChapter(page: Page, title: string) {
  await page.getByRole("button", { name: "Add Chapter" }).first().click();
  const modal = page.locator(".rinch-modal__body:visible");
  await modal.locator("input[placeholder='Enter chapter title']").fill(title);
  await modal.getByRole("button", { name: "Add", exact: true }).click();
  // The chapter shows up in the sidebar.
  await expect(
    page.locator(".sidebar-chapter-name", { hasText: title }),
  ).toBeVisible();
}

/**
 * Open a chapter in the editor by its sidebar name; waits until it's editable.
 *
 * The editor node always exists but only flips to `contenteditable=true` once a
 * chapter switch has loaded its content. Immediately after a chapter is created
 * the click handler can briefly miss, so retry the click until the editor is
 * actually editable.
 */
export async function openChapter(page: Page, title: string) {
  const item = page.locator(".sidebar-chapter-item", { hasText: title });
  await expect(item).toBeVisible();
  await item.click();
  // Wait for the chapter switch to load and flip the editor editable.
  await page.locator('#editor-main[contenteditable="true"]').waitFor({ timeout: 15_000 });
}
