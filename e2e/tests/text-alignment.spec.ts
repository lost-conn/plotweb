import { test, expect, Locator, Page } from "@playwright/test";
import {
  addChapter,
  createBook,
  openChapter,
  registerNewUser,
  returnChrome,
  typeInEditor,
} from "./helpers";

/**
 * Paragraph text alignment: the four toolbar buttons, the model attribute they
 * write, and the three surfaces it has to survive to — the live editor, a
 * reload (so it reached the server), and the reader.
 *
 * The buttons run rinch's `setTextAlign{Left,Center,Right,Justify}` commands,
 * which set a `text_align` attribute on every paragraph/heading under the
 * selection. Two different renderers then project that attribute:
 *
 *   - the **editor** view sets it imperatively (`set_style("text-align", …)`),
 *     so the browser's CSSOM serializes the style attribute as
 *     `text-align: center;`;
 *   - the **reader** goes through rinch's HTML serializer, which writes the
 *     compact `style="text-align:center"` into markup that is then injected
 *     with `innerHTML`.
 *
 * Both spellings matter to `editor_utils.rs`'s CSS (the rules that suppress the
 * first-line indent on centred/right-aligned paragraphs key off the inline
 * style), which is exactly why these assertions read the **computed** style
 * rather than the attribute: computed style is the one thing both paths agree
 * on, and it is what the author actually sees.
 *
 * `ActionIcon` has no class or attribute prop, so each alignment button is
 * wrapped in a `span.toolbar-align[data-align=…]` that carries the hook — the
 * only stable selector for a toolbar whose buttons are otherwise distinguished
 * by icon alone.
 */

/**
 * One alignment's button in the **chapter** editor's toolbar.
 *
 * `editor_toolbar` is shared: the chapter editor and the note editor each mount
 * one, and the note pane stays in the DOM while hidden — so an unscoped
 * `.toolbar` matches twice. `.editor-layout` is the chapter editor's own
 * wrapper (`panes/editor.rs`) and is the thing that tells them apart.
 */
const alignButton = (page: Page, align: string): Locator =>
  page.locator(
    `.editor-layout .toolbar .toolbar-align[data-align="${align}"] .rinch-action-icon`,
  );

/**
 * Whether an alignment button is showing its "on" highlight.
 *
 * `fmt_button` draws the active state by flipping the ActionIcon's `variant`
 * from `subtle` to `light`, which lands as the class below — the same mechanism
 * the Bold/Italic/H1 buttons use.
 */
async function isActive(page: Page, align: string): Promise<boolean> {
  const cls =
    (await alignButton(page, align).getAttribute("class")) ?? "";
  return cls.includes("rinch-action-icon--light");
}

/** The computed `text-align` of the first paragraph in the editor surface. */
const editorParagraph = (page: Page): Locator =>
  page.locator("#editor-main [data-pm-editor] p").first();

async function computedAlign(locator: Locator): Promise<string> {
  return locator.evaluate((el) => getComputedStyle(el).textAlign);
}

/** The computed `text-indent` of an element, in px. */
async function computedIndent(locator: Locator): Promise<string> {
  return locator.evaluate((el) => getComputedStyle(el).textIndent);
}

test("centering a paragraph holds in the editor, across a reload, and in the reader", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Alignment Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "This paragraph belongs on the centre line.");

  const para = editorParagraph(page);
  await expect(para).toContainText("centre line");

  // Fresh prose starts left-aligned, and the Left button says so. "left" is
  // stored as the *absence* of the attribute, so this also pins the default
  // that `text_align_of` has to invent.
  expect(await computedAlign(para)).toBe("left");
  expect(await isActive(page, "left")).toBe(true);
  expect(await isActive(page, "center")).toBe(false);

  // The toolbar faded and is pointer-events:none while `is-writing` (chrome
  // fade-away), which now persists until asked back — bring it back before
  // clicking it, same as an author reaching for the mouse would. Escape
  // doesn't touch the editor's selection, so the caret stays put for the
  // alignment command below.
  await returnChrome(page);
  await alignButton(page, "center").click();

  // The editor repaints the paragraph...
  await expect.poll(async () => computedAlign(para)).toBe("center");
  // ...and the toolbar follows the caret: exactly one of the four is lit.
  await expect.poll(async () => isActive(page, "center")).toBe(true);
  expect(await isActive(page, "left")).toBe(false);
  expect(await isActive(page, "right")).toBe(false);
  expect(await isActive(page, "justify")).toBe(false);

  // Past the 3s autosave debounce, so the alignment is PUT to the server rather
  // than living only in this tab.
  await page.waitForTimeout(4000);

  await page.reload();
  await openChapter(page, "Chapter One");
  const reloaded = editorParagraph(page);
  await expect(reloaded).toContainText("centre line");
  await expect.poll(async () => computedAlign(reloaded)).toBe("center");
  // The toolbar has to re-derive the state from the reloaded document, not from
  // a signal left over from the click.
  await reloaded.click();
  await expect.poll(async () => isActive(page, "center")).toBe(true);

  // ── The reader ──────────────────────────────────────────────────────
  // The author preview is the same reader component as `/read/{token}` and
  // needs no beta link, so it is the cheapest way to assert the *second*
  // renderer (HTML serializer + `innerHTML`) also carries the alignment.
  await page.goto(`/preview/${bookId}`);
  const readerItem = page.locator(".reader-chapter-item", { hasText: "Chapter One" });
  if (!(await readerItem.isVisible().catch(() => false))) {
    await page.locator(".reader-topbar .rinch-action-icon").first().click();
  }
  await readerItem.click();

  const readerPara = page.locator("#reader-content p", { hasText: "centre line" }).first();
  await expect(readerPara).toBeVisible();
  expect(await computedAlign(readerPara)).toBe("center");
});

test("Left restores the default, and centring drops the first-line indent", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Alignment Round Trip");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "Back and forth across the axis.");
  const para = editorParagraph(page);
  await expect(para).toContainText("across the axis");

  // The toolbar is still collapsed from typing above (chrome fade-away, no
  // idle return) — ask for it back before the first click reaches it.
  await returnChrome(page);

  // Right, then justify, then back to left: each is exclusive, and left has to
  // be reachable again rather than being a one-way door.
  //
  // Each alignment command is itself a document edit, so it re-triggers
  // `on_change` → `start_writing()` and re-collapses the toolbar the moment
  // the click lands — same as any other keystroke. A real author's next click
  // is preceded by a fresh mouse move that clears it again; Playwright has to
  // ask explicitly (`returnChrome`) before every one of these chained clicks.
  await alignButton(page, "right").click();
  await expect.poll(async () => computedAlign(para)).toBe("right");
  await expect.poll(async () => isActive(page, "right")).toBe(true);

  await returnChrome(page);
  await alignButton(page, "justify").click();
  await expect.poll(async () => computedAlign(para)).toBe("justify");
  await expect.poll(async () => isActive(page, "justify")).toBe(true);
  expect(await isActive(page, "right")).toBe(false);

  await returnChrome(page);
  await alignButton(page, "left").click();
  await expect.poll(async () => computedAlign(para)).toBe("left");
  await expect.poll(async () => isActive(page, "left")).toBe(true);
  expect(await isActive(page, "justify")).toBe(false);

  // ── First-line indent vs. alignment ─────────────────────────────────
  // With a paragraph indent configured, a centred paragraph must lose the
  // indent (it would shove the first line off the axis) while a justified one
  // keeps it. The indent normally comes from the Typography pane; setting it on
  // the book directly is the same `font_settings` field with none of the
  // pane's imperative select-building in the way.
  const resp = await page.request.put(`/api/books/${bookId}`, {
    data: { font_settings: { paragraph_indent: 32.0 } },
  });
  expect(resp.ok()).toBeTruthy();

  await page.reload();
  await openChapter(page, "Chapter One");
  const indented = editorParagraph(page);
  await expect(indented).toContainText("across the axis");

  // Left-aligned: the indent applies.
  await expect.poll(async () => computedIndent(indented)).toBe("32px");

  // Centred: the indent is suppressed.
  await indented.click();
  await alignButton(page, "center").click();
  await expect.poll(async () => computedAlign(indented)).toBe("center");
  await expect.poll(async () => computedIndent(indented)).toBe("0px");

  // Justified: a justified paragraph is an ordinary indented paragraph whose
  // lines are stretched, so the indent comes back. The centre click just above
  // was itself an edit, so the toolbar is collapsed again — ask it back.
  await returnChrome(page);
  await alignButton(page, "justify").click();
  await expect.poll(async () => computedAlign(indented)).toBe("justify");
  await expect.poll(async () => computedIndent(indented)).toBe("32px");
});
