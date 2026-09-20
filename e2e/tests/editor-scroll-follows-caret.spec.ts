import { test, expect, Page } from "@playwright/test";
import { createBook, openChapter, registerNewUser, seedLongChapter } from "./helpers";

/**
 * The manuscript editor's scroller follows the caret.
 *
 * The editor is model-first (`rinch-editor-view`): its surface is not a
 * `contenteditable`, so the browser never auto-scrolls to a moving caret the
 * way it does for a native editable. rinch paints the caret itself as an
 * absolutely-positioned `[data-pm-caret]` div and, on every move, raises a
 * `ScrollSelectionIntoView` request that the runtime fulfils with a minimal
 * ("nearest") scroll of the closest `overflow-y: auto` ancestor — here
 * `.editor-scroll`. Before that request was honoured, holding ArrowDown or
 * typing past the bottom edge left the caret off screen while the page stayed
 * put.
 *
 * Three things are pinned:
 *  1. moving the caret below the fold with ArrowDown scrolls it into view;
 *  2. typing past the bottom edge (Enter after Enter) keeps the caret visible;
 *  3. the follow is gated on caret *movement*, so wheel-scrolling away from a
 *     resting caret is not undone by the caret's blink (which rewrites the same
 *     overlay element's `display` several times a second).
 */

const SCROLLER = ".editor-scroll";
const CARET = "#editor-main [data-pm-caret]";

/**
 * Where the caret is painted, relative to the scroller's viewport, read from
 * the overlay's own `left/top/height` styles rather than its bounding box: the
 * blink hides the element (`display: none`) half the time, which would zero a
 * `getBoundingClientRect` reading but leaves the positioning styles intact.
 * Returns `null` until the caret has been placed at all.
 */
async function caretBand(page: Page): Promise<{ top: number; bottom: number; viewTop: number; viewBottom: number } | null> {
  return page.evaluate(
    ([caretSel, scrollerSel]) => {
      const caret = document.querySelector(caretSel) as HTMLElement | null;
      const scroller = document.querySelector(scrollerSel) as HTMLElement | null;
      if (!caret || !scroller || !caret.style.top) return null;
      // The caret is positioned against the editor container (`position:
      // relative` in rinch's own stylesheet), which is its parent.
      const host = caret.parentElement as HTMLElement;
      const hostTop = host.getBoundingClientRect().top;
      const top = hostTop + parseFloat(caret.style.top);
      const bottom = top + parseFloat(caret.style.height);
      const view = scroller.getBoundingClientRect();
      return { top, bottom, viewTop: view.top, viewBottom: view.bottom };
    },
    [CARET, SCROLLER],
  );
}

async function expectCaretVisible(page: Page) {
  await expect
    .poll(async () => {
      const band = await caretBand(page);
      if (!band) return "no caret";
      // A pixel of slack either side: the caret's own `top` is rounded.
      const inside = band.top >= band.viewTop - 1 && band.bottom <= band.viewBottom + 1;
      return inside ? "visible" : `caret ${band.top}-${band.bottom} outside ${band.viewTop}-${band.viewBottom}`;
    })
    .toBe("visible");
}

const scrollTop = (page: Page) => page.locator(SCROLLER).evaluate((el) => el.scrollTop);

test("arrow keys and typing past the bottom keep the caret in view; blink does not fight a manual scroll", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Long Scroll Novel");
  await seedLongChapter(page, bookId, "Chapter One", 60);
  await openChapter(page, "Chapter One");

  // Precondition: the chapter overflows its scroller, or nothing here is tested.
  // Polled, not read once: `openChapter` resolves when the editor surface is
  // visible, which is before the seeded body has been loaded into it.
  await expect
    .poll(() => page.locator(SCROLLER).evaluate((el) => el.scrollHeight > el.clientHeight + 200))
    .toBe(true);

  // Caret at the very top, scroller at rest.
  await page.locator("#editor-main [data-pm-editor] p").first().click();
  await expect.poll(() => scrollTop(page)).toBe(0);
  await expectCaretVisible(page);

  // 1. ArrowDown past the fold. Sixty paragraphs of two-ish lines each is far
  //    more than any viewport shows; 80 steps lands well below the first fold.
  await page.keyboard.press("ArrowDown", { delay: 10 });
  for (let i = 0; i < 79; i++) await page.keyboard.press("ArrowDown", { delay: 10 });
  await expectCaretVisible(page);
  const afterDown = await scrollTop(page);
  expect(afterDown).toBeGreaterThan(0);

  // ArrowUp all the way back: the scroller follows upward too.
  for (let i = 0; i < 80; i++) await page.keyboard.press("ArrowUp", { delay: 10 });
  await expectCaretVisible(page);
  await expect.poll(() => scrollTop(page)).toBeLessThan(afterDown);

  // 2. Typing past the bottom edge. Jump to the last paragraph (Playwright's
  //    click scrolls it into view itself) and keep opening new lines: each Enter
  //    moves the caret one line further down than the scroller was showing.
  await page.locator("#editor-main [data-pm-editor] p").last().click();
  await page.keyboard.press("End");
  const beforeTyping = await scrollTop(page);
  for (let i = 0; i < 12; i++) {
    await page.keyboard.press("Enter");
    await page.keyboard.type(`New line ${i + 1}`, { delay: 5 });
  }
  await expectCaretVisible(page);
  await expect.poll(() => scrollTop(page)).toBeGreaterThan(beforeTyping);

  // 3. Wheel away from the resting caret: the blink must not yank it back.
  await page.locator(SCROLLER).evaluate((el) => {
    el.scrollTop = 0;
  });
  await expect.poll(() => scrollTop(page)).toBe(0);
  // Several blink periods (rinch blinks at ~500ms).
  await page.waitForTimeout(1500);
  expect(await scrollTop(page)).toBe(0);

  // …and the next caret move brings it back into view. ArrowLeft, not
  // ArrowRight: the typing loop above leaves the caret at the absolute end of
  // the document, where ArrowRight is a no-op (nothing to move past) — since
  // the follow is gated on actual caret *movement*, a no-op key would prove
  // nothing here. ArrowLeft is guaranteed to move.
  await page.keyboard.press("ArrowLeft");
  await expectCaretVisible(page);
  await expect.poll(() => scrollTop(page)).toBeGreaterThan(0);
});
