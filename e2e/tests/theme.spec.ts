import { test, expect, Page } from "@playwright/test";
import { registerNewUser } from "./helpers";

/**
 * Dark-mode theme toggle coverage.
 *
 * rinch-web reflects the active theme through a single page-global
 * `<style data-rinch-theme>` element in `<head>` whose CSS text is rewritten on
 * every `store.dark_mode` change (rinch-web's `set_on_signal_change` ->
 * `setup_theme_css` -> web_document path). The default is dark.
 *
 * On the dashboard, `.dash-topbar-right` holds two ActionIcons: the FIRST is the
 * dark-mode toggle (`toggle_dark`), the second is logout. Clicking the first
 * flips the theme, which must rewrite the theme `<style>` CSS (dark -> light
 * produces different CSS-variable values) and repaint the body background.
 */

/** Read the current page-global theme CSS text. */
async function themeCss(page: Page): Promise<string> {
  return page.evaluate(
    () => document.querySelector("[data-rinch-theme]")?.textContent ?? "",
  );
}

/** Read the computed body background color. */
async function bodyBg(page: Page): Promise<string> {
  return page.evaluate(
    () => getComputedStyle(document.body).backgroundColor,
  );
}

test("dark-mode toggle rewrites the page-global theme style", async ({ page }) => {
  await registerNewUser(page);
  await page.goto("/");

  const toggle = page.locator(".dash-topbar-right .rinch-action-icon").first();
  await expect(toggle).toBeVisible();

  // The theme style is present and non-empty at the default (dark) theme.
  const cssBefore = await themeCss(page);
  const bgBefore = await bodyBg(page);
  expect(cssBefore.length).toBeGreaterThan(0);

  // Toggle: dark -> light. The theme CSS and body background must both change.
  await toggle.click();
  await expect.poll(() => themeCss(page)).not.toBe(cssBefore);
  await expect.poll(() => bodyBg(page)).not.toBe(bgBefore);

  const cssAfter = await themeCss(page);

  // Toggle back: light -> dark. It flips again, back to the original CSS.
  await toggle.click();
  await expect.poll(() => themeCss(page)).not.toBe(cssAfter);
  await expect.poll(() => themeCss(page)).toBe(cssBefore);
  await expect.poll(() => bodyBg(page)).toBe(bgBefore);
});
