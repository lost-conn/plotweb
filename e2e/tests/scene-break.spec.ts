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
 * Scene break (horizontal rule) in the editor and reader.
 *
 * The toolbar button runs rinch's name-only `insertHorizontalRule` command,
 * which drops an atomic `horizontal_rule` node at the caret. It renders as a
 * plain `<hr>` in both the editor (`[data-pm-editor] hr`, imperative DOM) and
 * the reader (rinch's HTML serializer writes `<hr>`, then `innerHTML`) — see
 * the `#editor-main [data-pm-editor] hr` / `.reader-content hr` rules in
 * `editor_utils.rs` / `reader.rs` for the shared "short centred rule" look
 * (30% width, auto margins).
 *
 * The button has no active/tooltip hook (`toolbar_button`'s `tooltip` param is
 * decorative only — see its unused `let _ = tooltip;`), so it's targeted by
 * its icon's path data instead: `TablerIcon::SeparatorHorizontal` draws a
 * unique `M8 8l4 -4l4 4` arrow that no other toolbar icon shares.
 */
const sceneBreakButton = (page: Page) =>
  page.locator('.editor-layout .toolbar .rinch-action-icon:has(path[d="M8 8l4 -4l4 4"])');

const editorHr = (page: Page) => page.locator("#editor-main [data-pm-editor] hr");

/**
 * The paragraph immediately after the hr in document order. Not a plain CSS
 * `hr + p`: `[data-pm-editor]` also renders transient overlay divs (blinking
 * caret, node-selection box) as ordinary DOM siblings, which can land right
 * between the hr and the next paragraph and defeat an adjacent-sibling
 * selector. XPath's `following-sibling::p[1]` walks past those since it
 * filters by tag name, not position.
 */
const pAfterHr = (page: Page) => editorHr(page).locator("xpath=following-sibling::p[1]");

test("scene break splits two paragraphs, survives a reload, and renders in the reader", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Scene Break Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "The first scene ends here.");
  await page.keyboard.press("Enter");

  // Typing (and the Enter that follows) collapsed the chrome — bring the
  // toolbar back before clicking it, same as text-alignment.spec.ts.
  await returnChrome(page);
  await expect(sceneBreakButton(page)).toBeVisible();
  await sceneBreakButton(page).click();

  // `insertHorizontalRule` leaves the caret in the trailing empty paragraph,
  // but `typeInEditor`'s blanket click on the whole `[data-pm-editor]` surface
  // would land on/near the freshly-inserted `hr` (a zero-height atom) and
  // select it as a node — the next keystroke would then replace the rule
  // itself instead of typing into the paragraph after it. Click that
  // paragraph specifically instead of reusing `typeInEditor` here.
  await page.locator("#editor-main [data-pm-editor] p").last().click();
  await page.keyboard.type("The second scene begins here.");

  // Exactly one hr, sitting between the two paragraphs. `[data-pm-editor]`'s
  // direct children also include transient overlay divs (the blinking-caret
  // and node-selection boxes, `data-pm-caret`/`data-pm-selected`) interleaved
  // with the real document nodes, so filter down to elements carrying
  // `data-pm-type` (every actual block node has one) before checking order.
  await expect(editorHr(page)).toHaveCount(1);
  const order = await page.locator("#editor-main [data-pm-editor]").evaluate((root) => {
    return Array.from(root.children)
      .filter((el) => el.hasAttribute("data-pm-type"))
      .map((el) => el.tagName.toLowerCase());
  });
  const hrIndex = order.indexOf("hr");
  expect(hrIndex).toBeGreaterThan(0);
  expect(order[hrIndex - 1]).toBe("p");
  expect(order[hrIndex + 1]).toBe("p");

  await expect(editorHr(page).locator("xpath=preceding-sibling::p[1]")).toContainText(
    "first scene ends here",
  );
  await expect(pAfterHr(page)).toContainText("second scene begins here");

  // Short, centred rule: ~30% of the editor content's width.
  const hrBox = await editorHr(page).boundingBox();
  const containerBox = await page.locator("#editor-main [data-pm-editor]").boundingBox();
  expect(hrBox).not.toBeNull();
  expect(containerBox).not.toBeNull();
  const ratio = hrBox!.width / containerBox!.width;
  expect(ratio).toBeGreaterThan(0.2);
  expect(ratio).toBeLessThan(0.4);

  // Wait for the autosave round trip to actually land on the server, rather
  // than trusting the "Saved" indicator or a fixed sleep past the 3s debounce
  // (see text-alignment.spec.ts): the debounce timer is a single reschedule
  // per edit, but this test fires two edits close together (the button click,
  // then the second paragraph's keystrokes), and the indicator can flip back
  // to "Saved" off an earlier, now-superseded autosave. Polling the chapter
  // GET directly confirms the *content* the server actually has, not just
  // that some save happened.
  const listResp = await page.request.get(`/api/books/${bookId}/chapters`);
  const chapterId = ((await listResp.json()) as Array<{ id: string; title: string }>).find(
    (c) => c.title === "Chapter One",
  )?.id;
  expect(chapterId).toBeTruthy();
  await expect
    .poll(
      async () => {
        const resp = await page.request.get(`/api/books/${bookId}/chapters/${chapterId}`);
        const chapter = (await resp.json()) as { content: string };
        return chapter.content;
      },
      { timeout: 15_000 },
    )
    .toContain("second scene begins here");

  await page.reload();
  await openChapter(page, "Chapter One");
  await expect(editorHr(page)).toHaveCount(1);
  await expect(pAfterHr(page)).toContainText("second scene begins here");

  // ── The reader ──────────────────────────────────────────────────────
  await page.goto(`/preview/${bookId}`);
  const readerItem = page.locator(".reader-chapter-item", { hasText: "Chapter One" });
  if (!(await readerItem.isVisible().catch(() => false))) {
    await page.locator(".reader-topbar .rinch-action-icon").first().click();
  }
  await readerItem.click();

  await expect(page.locator("#reader-content")).toContainText("second scene begins here");
  await expect(page.locator("#reader-content hr")).toHaveCount(1);
});
