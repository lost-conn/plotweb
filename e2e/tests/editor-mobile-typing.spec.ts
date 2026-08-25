import { test, expect, Page } from "@playwright/test";
import { createBook, registerNewUser } from "./helpers";

/**
 * Typing in the manuscript editor from an **on-screen keyboard**.
 *
 * The editor is model-first (`rinch-editor-view`): the surface is deliberately not a
 * `contenteditable`, and every physical key is routed through a document-level
 * `keydown` listener. Software keyboards do not speak that protocol — Android reports
 * printable keys as `key: "Unidentified"` / `keyCode: 229` and delivers the actual text
 * on `beforeinput` alone. With no `beforeinput` listener the editor stayed empty no
 * matter what the user typed on a phone; rinch now maps `inputType` onto the same
 * handle calls the keymap uses.
 *
 * Playwright cannot summon a real soft keyboard, so this drives the exact event
 * sequence one emits: an uninformative `keydown` (which must NOT be treated as text or
 * swallowed) followed by the `beforeinput` that carries the edit. Focus, by contrast,
 * is driven by genuine CDP touch events — the tap that raises the keyboard has to land
 * on the hidden capture target, or a real phone never gets this far.
 */

/** The hidden `<textarea>` the editor uses as the browser's focused editable. */
const CAPTURE = "[data-pm-capture]";

/** Tap at a viewport point with real touch events (pointer + compatibility mouse). */
async function tap(page: Page, cdp: any, selector: string) {
  const box = await page.locator(selector).boundingBox();
  if (!box) throw new Error(`no box for ${selector}`);
  const x = box.x + box.width / 2;
  const y = box.y + Math.min(box.height / 2, 24);
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y }] });
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
}

/**
 * Replay what an Android soft keyboard sends for `text`: per character, a `keydown`
 * with no usable identity, then the `beforeinput` that actually carries it.
 */
async function softKeyboardType(page: Page, text: string) {
  await page.evaluate((t) => {
    const ta = document.querySelector("[data-pm-capture]");
    if (!ta) throw new Error("no capture target — the editor never took focus");
    for (const ch of t) {
      ta.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Unidentified",
          keyCode: 229,
          bubbles: true,
          cancelable: true,
        }),
      );
      ta.dispatchEvent(
        new InputEvent("beforeinput", {
          inputType: "insertText",
          data: ch,
          bubbles: true,
          cancelable: true,
        }),
      );
    }
  }, text);
}

/** Send one non-text `beforeinput` (Enter, Backspace, undo — all `inputType`s). */
async function softKeyboardEdit(page: Page, inputType: string) {
  await page.evaluate((t) => {
    const ta = document.querySelector("[data-pm-capture]");
    if (!ta) throw new Error("no capture target");
    ta.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Unidentified",
        keyCode: 229,
        bubbles: true,
        cancelable: true,
      }),
    );
    ta.dispatchEvent(
      new InputEvent("beforeinput", { inputType: t, bubbles: true, cancelable: true }),
    );
  }, inputType);
}

const editorText = (page: Page) =>
  page.locator("#editor-main [data-pm-editor]").innerText();

/**
 * Dismiss the mobile book sidebar if it is open, by tapping the backdrop beside it —
 * the drawer covers the hamburger that would otherwise toggle it.
 */
async function closeSidebarDrawer(page: Page) {
  const backdrop = page.locator(".sidebar-backdrop.open");
  if ((await backdrop.count()) === 0) return;
  const size = page.viewportSize();
  if (!size) throw new Error("no viewport");
  await backdrop.click({ position: { x: size.width - 20, y: size.height / 2 } });
  await expect(backdrop).toHaveCount(0);
}

/**
 * Add a chapter and wait for it in the *main pane* list. (The shared `addChapter`
 * helper waits on the sidebar copy, which is inside the dismissed drawer here.)
 */
async function addMobileChapter(page: Page, title: string) {
  await page.getByRole("button", { name: "Add Chapter" }).first().click();
  const modal = page.locator(".rinch-modal__body:visible");
  await modal.locator("input[placeholder='Enter chapter title']").fill(title);
  await modal.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.locator(".chapter-item", { hasText: title })).toBeVisible();
}

test("mobile: an on-screen keyboard types, deletes, and splits paragraphs", async ({
  browser,
}) => {
  const ctx = await browser.newContext({
    baseURL: "http://localhost:3000",
    viewport: { width: 390, height: 780 },
    hasTouch: true,
    isMobile: true,
  });
  const page = await ctx.newPage();
  const cdp = await ctx.newCDPSession(page);

  await registerNewUser(page);
  await createBook(page, "Phone Novel");

  // At this width the book sidebar is a drawer over a backdrop, and it opens with the
  // book — everything in the main pane is behind it until it's dismissed.
  await closeSidebarDrawer(page);
  await addMobileChapter(page, "Chapter One");
  // Open the chapter from the main pane's list (the sidebar's copy is behind the
  // drawer, which is exactly how a phone user reaches it).
  await page.locator(".chapter-item", { hasText: "Chapter One" }).first().click();
  await page
    .locator("#editor-main [data-pm-editor]")
    .waitFor({ state: "visible", timeout: 15_000 });

  // A tap inside the editor must focus the hidden capture target — that focus call is
  // what raises the on-screen keyboard, and it has to happen inside the touch gesture.
  await tap(page, cdp, "#editor-main [data-pm-editor]");
  await expect
    .poll(() =>
      page.evaluate(
        () => document.activeElement?.hasAttribute("data-pm-capture") ?? false,
      ),
    )
    .toBe(true);

  // Typing.
  await softKeyboardType(page, "Hello phone");
  await expect.poll(() => editorText(page)).toContain("Hello phone");

  // Backspace.
  await softKeyboardEdit(page, "deleteContentBackward");
  await expect.poll(() => editorText(page)).toContain("Hello phon");
  expect(await editorText(page)).not.toContain("Hello phone");

  // Enter splits the block, and typing continues in the new one.
  await softKeyboardEdit(page, "insertParagraph");
  await softKeyboardType(page, "second");
  await expect
    .poll(() => page.locator("#editor-main [data-pm-editor] p").count())
    .toBeGreaterThan(1);
  await expect.poll(() => editorText(page)).toContain("second");

  // Word delete (the long-press / swipe-back gesture).
  await softKeyboardEdit(page, "deleteWordBackward");
  await expect.poll(() => editorText(page)).not.toContain("second");

  // Nothing leaked into the capture textarea: it is the clipboard's source on `copy`,
  // so text left there would hijack the next copy.
  expect(
    await page.evaluate(
      () => (document.querySelector("[data-pm-capture]") as HTMLTextAreaElement).value,
    ),
  ).toBe("");

  await ctx.close();
});
