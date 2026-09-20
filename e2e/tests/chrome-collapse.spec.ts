import { test, expect, Page } from "@playwright/test";
import {
  addChapter,
  createBook,
  openChapter,
  registerNewUser,
  typeInEditor,
} from "./helpers";

/**
 * Chrome fade-away while typing in the chapter editor.
 *
 * `.book-workspace.is-writing` / `.editor-layout.is-writing` are set for as
 * long as the author is actively typing (see `editor_writing` in
 * `pages/book/mod.rs`). On desktop (>= 769px, the default Playwright
 * viewport used here) the `.book-sidebar` (250px) and, when open, the
 * `.editor-feedback-sidebar` (300px) collapse all the way to zero width over
 * a 320ms transition (`--pw-dur-slow`) so `.editor-content` re-centers on the
 * viewport; the topbar and toolbar fade with opacity, but `.editor-footer`
 * (word count + save indicator) stays fully visible and interactive
 * throughout. The chrome comes back ONLY on pointer move or Escape — the
 * previous 2.5s idle-return timer was removed, so once collapsed it stays
 * collapsed indefinitely while the author keeps typing or simply stops
 * touching the mouse.
 */

const bookWorkspace = (page: Page) => page.locator(".book-workspace");
const editorLayout = (page: Page) => page.locator(".editor-layout");
const bookSidebar = (page: Page) => page.locator(".book-sidebar");
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

test("Escape brings the chrome back immediately", async ({ page }) => {
  await openFreshChapter(page);

  await typeInEditor(page, "Escape should restore the chrome.");
  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  await page.keyboard.press("Escape");

  await expect(bookWorkspace(page)).not.toHaveClass(/is-writing/);
  await expect(editorLayout(page)).not.toHaveClass(/is-writing/);
  await expect.poll(async () => bookSidebar(page).evaluate((el) => getComputedStyle(el).width)).toBe("250px");
});

test("moving the mouse brings the chrome back", async ({ page }) => {
  await openFreshChapter(page);

  // Click (to focus the editor surface) and only THEN type, so the click's own
  // pointer position doesn't count as "the" mouse move the assertion below
  // needs to be distinct from.
  const surface = page.locator("#editor-main [data-pm-editor]");
  await surface.click();
  await page.keyboard.type("Moving the mouse should restore the chrome.");

  await expect(bookWorkspace(page)).toHaveClass(/is-writing/);

  // Two distinct, well-separated points — `page.keyboard.type` does not itself
  // dispatch mousemove, so the collapsed state is untouched until these fire.
  // The handler has no minimum-distance threshold (it reacts to any real
  // mousemove while collapsed), but a large jump rules out any ambiguity with
  // residual coordinates from the click above.
  await page.mouse.move(20, 20);
  await page.mouse.move(600, 400);

  await expect(bookWorkspace(page)).not.toHaveClass(/is-writing/);
  await expect(editorLayout(page)).not.toHaveClass(/is-writing/);
});
