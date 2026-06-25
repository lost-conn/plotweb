import { test, expect } from "@playwright/test";
import { login, logout, registerNewUser, uniqueUser } from "./helpers";

test("redirects to login when unauthenticated", async ({ page }) => {
  await page.goto("/");
  await expect(page).toHaveURL(/\/login/);
  await expect(page.getByText("Welcome back")).toBeVisible();
});

test("register, logout, and log back in", async ({ page }) => {
  const { username, password } = await registerNewUser(page);

  // Reloading keeps the session (cookie) — still on the dashboard, not login.
  await page.goto("/");
  await expect(page).not.toHaveURL(/\/login/);
  await expect(page.getByText(username, { exact: false }).first()).toBeVisible();

  // Log out via the topbar, then log back in.
  await logout(page);
  await login(page, username, password);
  await expect(page).not.toHaveURL(/\/login/);
  await expect(page.getByText(username, { exact: false }).first()).toBeVisible();
});

test("login with wrong password shows an error and stays on login", async ({ page }) => {
  const { username } = await registerNewUser(page);
  await logout(page);
  await login(page, username, "totally-wrong");
  await expect(page).toHaveURL(/\/login/);
  await expect(page.getByText(/error/i).first()).toBeVisible();
});

test("login with unknown user is rejected", async ({ page }) => {
  await login(page, uniqueUser("ghost"), "whatever123");
  await expect(page).toHaveURL(/\/login/);
});
