import { test, expect, Page } from "@playwright/test";
import {
  addChapter,
  createBook,
  openChapter,
  registerNewUser,
  returnChrome,
  typeInEditor,
} from "./helpers";

/**
 * Chrome fade-away while typing in the chapter editor.
 *
 * `.book-workspace.is-writing` / `.editor-layout.is-writing` are set for as
 * long as the author is actively typing (see `editor_writing` in
 * `pages/book/mod.rs`) *and* the per-device "Fade chrome while writing" switch
 * (Typography pane, `#pw-chrome-fade`) is on. On desktop (>= 769px, the
 * default Playwright viewport used here) the `.book-sidebar` (250px) and,
 * when open, the `.editor-feedback-sidebar` (300px) collapse all the way to
 * zero width over a 320ms transition (`--pw-dur-slow`) so `.editor-content`
 * re-centers on the viewport; the `.editor-topbar` and `.toolbar` collapse to
 * zero height the same way (fading with opacity too), and so does the "Notes
 * here" row (`.chapter-backlinks`) when a chapter has any. `.editor-footer`
 * (word count + save indicator) stays fully visible and interactive
 * throughout.
 *
 * The chrome comes back on Escape, or on a pointer move into a *reveal zone*
 * near where the collapsed chrome lives at rest — a strip down the left edge,
 * a band across the top, a strip down the right edge (see
 * `pages/book/chrome_zone.rs`). A move that stays over the prose in the
 * middle of the screen is a no-op — that's the whole point: reading back what
 * you just typed shouldn't un-collapse the chrome out from under you. There
 * is no idle-return timer, so once collapsed it stays collapsed indefinitely
 * while the author keeps typing, reads over the prose, or simply stops
 * touching the mouse.
 */

const bookWorkspace = (page: Page) => page.locator(".book-workspace");
const editorLayout = (page: Page) => page.locator(".editor-layout");
const bookSidebar = (page: Page) => page.locator(".book-sidebar");
const editorTopbar = (page: Page) => page.locator(".editor-topbar");
// `.toolbar` is shared with the (always-mounted, hidden) note editor's own
// toolbar — scope to the chapter editor's `.editor-layout` the same way
// `editorContent` above scopes `.editor-content` to `#editor-main`.
const toolbar = (page: Page) => page.locator(".editor-layout .toolbar");
const editorFooter = (page: Page) => page.locator(".editor-footer");
// `.editor-content` is shared with the (always-mounted, hidden) note editor's
// `#note-editor-main` — scope to the chapter editor's own id.
const editorContent = (page: Page) => page.locator("#editor-main.editor-content");

/** Set up a book with one chapter open in the editor, ready to type into. */
async function openFreshChapter(page: Page) {
  await registerNewUser(page);
  await createBook(page, "Chrome Collapse Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");
}

test("typing collapses the sidebar and centers the prose, and it stays collapsed past the old idle window", async ({
  page,
}) => {
  await openFreshChapter(page);

  await typeInEditor(page, "The chrome should get out of the way now.");

  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);
  await expect(editorLayout(page)).toHaveClass(/is-writing/);

  // Past the 320ms width/opacity transition.
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("0px");

  // The footer (word count + save indicator) stays fully visible the whole time.
  await expect(editorFooter(page)).toBeVisible();
  await expect(editorFooter(page)).toHaveCSS("opacity", "1");

  // With the sidebar gone, the prose column re-centers on the viewport rather
  // than staying offset into the space the sidebar used to occupy.
  const viewport = page.viewportSize();
  if (!viewport) throw new Error("no viewport size reported");
  await expect
    .poll(async () => {
      const box = await editorContent(page).boundingBox();
      if (!box) return NaN;
      const boxCenter = box.x + box.width / 2;
      return Math.abs(boxCenter - viewport.width / 2);
    })
    .toBeLessThan(8);

  // No idle timer brings the chrome back on its own — wait well past the old
  // 2.5s idle window and confirm the chrome is still collapsed.
  await page.waitForTimeout(3500);
  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);
  await expect(editorLayout(page)).toHaveClass(/is-writing/);
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("0px");
});

test("the topbar and toolbar collapse to zero height while writing, and restore on reveal", async ({ page }) => {
  await openFreshChapter(page);

  // Comfortably non-zero at rest.
  const restHeight = await editorTopbar(page).evaluate((el) => getComputedStyle(el).height);
  expect(parseFloat(restHeight)).toBeGreaterThan(10);

  await typeInEditor(page, "Shrink, topbar and toolbar.");
  await expect(editorLayout(page)).toHaveClass(/is-writing/);

  await expect.poll(async () => editorTopbar(page).evaluate((el) => getComputedStyle(el).height)).toBe("0px");
  await expect.poll(async () => toolbar(page).evaluate((el) => getComputedStyle(el).height)).toBe("0px");

  await returnChrome(page);
  await expect
    .poll(async () => editorTopbar(page).evaluate((el) => getComputedStyle(el).height))
    .not.toBe("0px");
  await expect.poll(async () => toolbar(page).evaluate((el) => getComputedStyle(el).height)).not.toBe("0px");
});

test("Escape brings the chrome back immediately", async ({ page }) => {
  await openFreshChapter(page);

  await typeInEditor(page, "Escape should restore the chrome.");
  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  await page.keyboard.press("Escape");

  await expect(bookWorkspace(page)).not.toHaveClass(/is-writing/);
  await expect(editorLayout(page)).not.toHaveClass(/is-writing/);
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("250px");
});

test("moving the mouse over the prose does not bring the chrome back", async ({ page }) => {
  await openFreshChapter(page);

  // Click (to focus the editor surface) and only THEN type, so the click's own
  // pointer position doesn't count as "the" mouse move the assertions below
  // need to be distinct from.
  const surface = page.locator("#editor-main [data-pm-editor]");
  await surface.click();
  await page.keyboard.type("Reading this back should not restore the chrome.");

  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  const viewport = page.viewportSize();
  if (!viewport) throw new Error("no viewport size reported");
  // Well clear of the left/top/right reveal zones (see `chrome_zone.rs`:
  // 56px edge strips, a 160px top band) — squarely in the prose.
  await page.mouse.move(viewport.width / 2, viewport.height / 2);
  await page.mouse.move(viewport.width / 2 - 40, viewport.height / 2 + 60);

  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);
  await expect(editorLayout(page)).toHaveClass(/is-writing/);
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("0px");
});

test("moving the mouse into an edge reveal zone brings the chrome back", async ({ page }) => {
  await openFreshChapter(page);

  const surface = page.locator("#editor-main [data-pm-editor]");
  await surface.click();
  await page.keyboard.type("Reaching for the sidebar should restore the chrome.");
  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  // Inside the left-edge strip (x <= 56px).
  await page.mouse.move(20, 400);

  await expect(bookWorkspace(page)).not.toHaveClass(/is-writing/);
  await expect(editorLayout(page)).not.toHaveClass(/is-writing/);
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("250px");
});

test("moving the mouse into the top band brings the chrome back", async ({ page }) => {
  await openFreshChapter(page);

  const surface = page.locator("#editor-main [data-pm-editor]");
  await surface.click();
  await page.keyboard.type("Reaching for the toolbar should restore the chrome.");
  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  const viewport = page.viewportSize();
  if (!viewport) throw new Error("no viewport size reported");
  // Inside the top band (y <= 160px), away from either edge strip.
  await page.mouse.move(viewport.width / 2, 30);

  await expect(bookWorkspace(page)).not.toHaveClass(/is-writing/);
  await expect(editorLayout(page)).not.toHaveClass(/is-writing/);
});

test("the chrome-fade switch, off, stops typing from collapsing anything", async ({ page }) => {
  await openFreshChapter(page);

  await typeInEditor(page, "Turn the fade off.");
  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  // Reach the Typography pane the same way `spellcheck.spec.ts` does — via the
  // footer tools strip, after bringing the (currently collapsed) chrome back.
  await returnChrome(page);
  await page.locator('.ws-tools .tool[data-tip="Typography"]').click();
  await page.locator("#pw-chrome-fade .rinch-switch").click();

  // Switching off while nothing is collapsed just leaves it that way; typing
  // more must never set `is-writing` again.
  await openChapter(page, "Chapter One");
  await typeInEditor(page, " Nothing should collapse now.");
  await page.waitForTimeout(500);
  await expect(bookWorkspace(page)).not.toHaveClass(/is-writing/);
  await expect(editorLayout(page)).not.toHaveClass(/is-writing/);
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("250px");
});

test("the chrome-fade switch stays off across a reload (per-device, not per-session)", async ({ page }) => {
  await openFreshChapter(page);

  await typeInEditor(page, "Collapse first, then turn the fade off.");
  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  await returnChrome(page);
  await page.locator('.ws-tools .tool[data-tip="Typography"]').click();
  await page.locator("#pw-chrome-fade .rinch-switch").click();

  // Give the fire-and-forget local-storage write (`crate::chrome_settings::
  // persist`) a moment to land before the reload that would otherwise race it.
  await page.waitForTimeout(500);
  await page.reload();
  await openChapter(page, "Chapter One");

  // Still off after the reload — typing must not collapse anything.
  await typeInEditor(page, "Still off after reload.");
  await page.waitForTimeout(500);
  await expect(bookWorkspace(page)).not.toHaveClass(/is-writing/);
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("250px");
});
