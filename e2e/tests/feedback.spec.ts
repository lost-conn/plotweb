import { test, expect, Page } from "@playwright/test";
import {
  addChapter,
  createBetaLink,
  createBook,
  openChapter,
  openFeedbackSidebar,
  registerNewUser,
} from "./helpers";

/**
 * Beta feedback coverage.
 *
 * Two flows are exercised end-to-end:
 *
 *  1. A public beta reader (no account, fresh browser context — the author's
 *     session cookie is absent) opens `/read/{token}`, selects a phrase in the
 *     prose, and submits a comment. We assert the comment is *persisted* on the
 *     server, not just optimistically shown.
 *
 *  2. The author replies to a seeded feedback item from the in-editor feedback
 *     sidebar, driving the reply textarea with real keystrokes to guard the
 *     rinch PR #94 behaviour: **Enter** submits the reply (and suppresses the
 *     newline), **Shift+Enter** inserts a newline and does NOT submit.
 */

const READER_ORIGIN = "http://localhost:3000";

// Distinctive prose with an easily-locatable, punctuation-free target phrase.
const PROSE =
  "The moonlight fell across the silver lake and nobody spoke a single word that night.";
const TARGET_PHRASE = "silver lake";

/** Write known prose into the open editor and wait past the 3s autosave debounce. */
async function writeChapterProse(page: Page, prose: string) {
  await page.locator("#editor-main").fill(prose);
  // Let the autosave debounce (3s) flush the content to git-backed storage.
  await page.waitForTimeout(4000);
}

/**
 * Drive a real text selection inside the reader's `#reader-content`, then
 * dispatch the same `mouseup` the app listens for.
 *
 * The reader can't be selected with a mouse drag reliably: `.reader-content` is
 * laid out in transformed CSS columns (paginated), so the target phrase is often
 * translated off-screen. Instead we build a DOM `Range` over the exact text node
 * that holds the phrase, install it as the window selection, and dispatch a
 * bubbling `mouseup` on the phrase's parent `<p>` — which is what the reader's
 * document-level `mouseup` handler keys off of (it reads `window.getSelection()`
 * and checks `target.closest('#reader-content')`).
 */
async function selectPhraseInReader(page: Page, phrase: string) {
  await page.evaluate((phrase) => {
    const root = document.querySelector("#reader-content");
    if (!root) throw new Error("#reader-content not found");
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let node: Node | null;
    let hit: { node: Text; idx: number } | null = null;
    while ((node = walker.nextNode())) {
      const idx = (node.textContent ?? "").indexOf(phrase);
      if (idx >= 0) {
        hit = { node: node as Text, idx };
        break;
      }
    }
    if (!hit) throw new Error("phrase not found in reader content: " + phrase);

    const range = document.createRange();
    range.setStart(hit.node, hit.idx);
    range.setEnd(hit.node, hit.idx + phrase.length);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);

    const rect = range.getBoundingClientRect();
    const target = hit.node.parentElement!;
    target.dispatchEvent(
      new MouseEvent("mouseup", {
        bubbles: true,
        cancelable: true,
        clientX: rect.left + rect.width / 2,
        clientY: rect.top,
      }),
    );
  }, phrase);
}

test("beta reader submits feedback on a selected text range, and it persists", async ({
  page,
  browser,
}) => {
  // ── Author: book + chapter with known prose + a beta link ────────────────
  await registerNewUser(page);
  const bookId = await createBook(page, "Beta Feedback Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");
  await writeChapterProse(page, PROSE);

  const token = await createBetaLink(page, bookId, "Alice");

  // The chapter id, straight from the public beta view.
  const viewResp = await page.request.get(`/api/beta/${token}`);
  const view = (await viewResp.json()) as { chapters: Array<{ id: string }> };
  const chapterId = view.chapters[0].id;

  // ── Reader: fresh context (no author cookie) proves the public path ──────
  const readerContext = await browser.newContext({ baseURL: READER_ORIGIN });
  const reader = await readerContext.newPage();
  try {
    await reader.goto(`/read/${token}`);

    // Open the chapter from the reader sidebar and wait for its prose to render.
    await reader.locator(".reader-chapter-item", { hasText: "Chapter One" }).click();
    await expect(reader.locator("#reader-content")).toContainText(TARGET_PHRASE);

    // Select the phrase → the feedback tooltip pops.
    await selectPhraseInReader(reader, TARGET_PHRASE);
    await expect(reader.locator(".feedback-tooltip.visible")).toBeVisible();

    // Type a comment and submit.
    const comment = "This imagery is gorgeous — keep it.";
    await reader.locator("#feedback-tooltip-textarea").fill(comment);
    await reader
      .locator(".feedback-tooltip-actions")
      .getByRole("button", { name: "Submit" })
      .click();

    // Persisted server-side (not just an optimistic UI insert).
    await expect
      .poll(async () => {
        const resp = await reader.request.get(`/api/beta/${token}/feedback`);
        const list = (await resp.json()) as Array<{
          comment: string;
          selected_text: string;
          chapter_id: string;
        }>;
        return list.some(
          (f) =>
            f.comment === comment &&
            f.selected_text === TARGET_PHRASE &&
            f.chapter_id === chapterId,
        );
      })
      .toBe(true);

    // And it shows up in the reader's feedback panel.
    await expect(reader.locator(".reader-feedback-list .feedback-comment")).toContainText(
      comment,
    );
    await expect(reader.locator(".reader-feedback-list .feedback-quote")).toContainText(
      TARGET_PHRASE,
    );
  } finally {
    await readerContext.close();
  }
});

test("author reply: Enter sends, Shift+Enter inserts a newline", async ({ page }) => {
  // ── Author: book + chapter + beta link ───────────────────────────────────
  await registerNewUser(page);
  const bookId = await createBook(page, "Reply Keys Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");
  await writeChapterProse(page, PROSE);

  const token = await createBetaLink(page, bookId, "Bob");

  const viewResp = await page.request.get(`/api/beta/${token}`);
  const view = (await viewResp.json()) as { chapters: Array<{ id: string }> };
  const chapterId = view.chapters[0].id;

  // ── Seed one feedback item via the public API (UI submit is covered above) ─
  const seed = await page.request.post(`/api/beta/${token}/feedback`, {
    data: {
      chapter_id: chapterId,
      selected_text: TARGET_PHRASE,
      context_block: PROSE,
      comment: "Is the lake symbolic here?",
    },
  });
  expect(seed.ok()).toBeTruthy();

  // Reload the book so the on-mount feedback fetch picks up the seeded item,
  // then open the editor for that chapter (the feedback toggle only appears
  // when the book has feedback and a chapter editor is active).
  await page.goto(`/book/${bookId}`);
  await openChapter(page, "Chapter One");
  await openFeedbackSidebar(page);

  const item = page.locator(".editor-feedback-sidebar .feedback-card");
  await expect(item).toContainText("Is the lake symbolic here?");
  const textarea = item.locator(".feedback-reply-input textarea");
  const replies = item.locator(".feedback-reply");

  // No replies yet.
  await expect(replies).toHaveCount(0);

  // ── Shift+Enter: inserts a newline, does NOT submit (real textarea) ──────
  // This half holds regardless of the rinch version: Shift+Enter is explicitly
  // left alone by rinch's keydown interceptor, so the browser's default newline
  // insertion applies and no submit fires.
  await textarea.click();
  await textarea.pressSequentially("line one");
  await textarea.press("Shift+Enter");
  await textarea.pressSequentially("line two");

  // The newline landed in the textarea value…
  await expect(textarea).toHaveValue("line one\nline two");
  // …and nothing was submitted (still zero replies, server-side too).
  await expect(replies).toHaveCount(0);
  {
    const resp = await page.request.get(`/api/books/${bookId}/feedback`);
    const list = (await resp.json()) as Array<{ replies: unknown[] }>;
    expect(list[0].replies).toHaveLength(0);
  }

  // ── Enter: submits the reply and suppresses the newline (real textarea) ──
  // rinch's document-level keydown handler fires the nearest ancestor's
  // `data-onsubmit` on Enter (no Shift), preventing the newline. Pressing Enter
  // in the still-focused reply textarea therefore sends the multi-line reply.
  await textarea.press("Enter");

  // The reply (multi-line content) now appears in the item's replies…
  await expect(replies).toHaveCount(1);
  await expect(replies.first()).toContainText("line one");
  await expect(replies.first()).toContainText("line two");
  // …the reply persisted server-side…
  await expect
    .poll(async () => {
      const resp = await page.request.get(`/api/books/${bookId}/feedback`);
      const list = (await resp.json()) as Array<{
        replies: Array<{ content: string }>;
      }>;
      return list[0]?.replies?.[0]?.content ?? "";
    })
    .toBe("line one\nline two");
  // …and the textarea cleared after a successful send.
  await expect(item.locator(".feedback-reply-input textarea")).toHaveValue("");
});
