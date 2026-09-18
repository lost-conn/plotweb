import { test, expect, Page } from "@playwright/test";
import {
  addChapter,
  createBook,
  createNote,
  openNotesPane,
  registerNewUser,
} from "./helpers";

/**
 * Leaving an edited note must write it, whichever way you leave.
 *
 * The note editor autosaves on an 800ms debounce, so the last few seconds of typing
 * live only in the editor model. Every exit therefore has to flush that pending write
 * *before* it changes `active_pane` — the debounce callback checks the pane and skips
 * the save once it has moved on.
 *
 * Only the back arrow did. Leaving through the sidebar's "Notes" entry (or a chapter,
 * or the dashboard, or sign-out) silently discarded the last ~800ms, so whether an
 * author's words survived depended on which exit they happened to take, with no
 * feedback either way. Worse, the debounced save cleared the dirty flag *before*
 * checking the pane, so a skipped save still marked the note clean and the text became
 * unrecoverable.
 *
 * PR #62 fixed exactly this shape for the *chapter* editor and missed the note path;
 * these tests walk each exit for the note editor. See `pages/book/flush.rs`.
 *
 * Each test types and then leaves *immediately*, inside the debounce window — that is
 * the whole point, and the `expect(writes).toEqual([])` before each exit proves the
 * window really was still open, so a pass can't come from the timer having fired.
 */

const NOTE_EDITOR = "#note-editor-main [data-pm-editor]";

/** Focus the note body and send real keystrokes (`.fill()` does nothing here). */
async function typeInNote(page: Page, text: string) {
  const surface = page.locator(NOTE_EDITOR);
  await surface.waitFor({ state: "visible", timeout: 15_000 });
  await surface.click();
  await page.keyboard.type(text);
}

/** Open a note from the notes tree by title and wait for its editor. */
async function openNoteFromTree(page: Page, title: string) {
  await page.locator(".notes-tree .note-card-title", { hasText: title }).click();
  await page.locator(NOTE_EDITOR).waitFor({ state: "visible", timeout: 15_000 });
}

/** Record every note-body PUT, so "did the exit write?" is a network fact. */
function watchNoteWrites(page: Page): string[] {
  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/notes\//.test(r.url())) {
      writes.push(new URL(r.url()).pathname);
    }
  });
  return writes;
}

/**
 * Reload and read the note back from the server — the only assertion that proves the
 * text was actually persisted rather than still sitting in a model this tab happens
 * to hold.
 */
async function expectNoteContains(page: Page, bookId: string, title: string, text: string) {
  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await openNoteFromTree(page, title);
  await expect(page.locator(NOTE_EDITOR)).toContainText(text);
}

/**
 * Set up a book with one note open and one chapter to leave towards, then type
 * `text` into the note and hand back the write log with the debounce still pending.
 */
async function typeIntoFreshNote(page: Page, bookTitle: string, text: string) {
  await registerNewUser(page);
  const bookId = await createBook(page, bookTitle);
  await addChapter(page, "Somewhere Else");
  await createNote(page, "The siege");
  await createNote(page, "Vess");

  await openNotesPane(page);
  await openNoteFromTree(page, "The siege");

  const writes = watchNoteWrites(page);
  await typeInNote(page, text);
  // The debounce has not fired: what follows is genuinely testing the flush, not the
  // timer beating us to it.
  expect(writes, "the 800ms debounce must still be pending").toEqual([]);
  return { bookId, writes };
}

test("the sidebar's Notes entry saves the note it is leaving", async ({ page }) => {
  // The reported bug, exactly: type, then leave through the sidebar rather than the
  // back arrow. `open_notes_pane` flushed the *chapter* editor and nothing else.
  const text = "Words typed just before reaching for the sidebar.";
  const { bookId, writes } = await typeIntoFreshNote(page, "Sidebar Exit", text);

  await openNotesPane(page);

  expect(writes, "leaving via the sidebar must write the note").not.toEqual([]);
  await expectNoteContains(page, bookId, "The siege", text);
});

test("opening a chapter saves the note it is leaving", async ({ page }) => {
  // `do_switch_chapter` saves the chapter it is leaving and knows nothing about the
  // note editor, so this exit dropped note edits too.
  const text = "Note text that must survive opening a chapter.";
  const { bookId, writes } = await typeIntoFreshNote(page, "Chapter Exit", text);

  await page.locator(".sidebar-chapter-item", { hasText: "Somewhere Else" }).click();
  await page
    .locator("#editor-main [data-pm-editor]")
    .waitFor({ state: "visible", timeout: 15_000 });

  expect(writes, "opening a chapter must write the note").not.toEqual([]);
  await expectNoteContains(page, bookId, "The siege", text);
});

test("following a rail link straight to another note saves the one being left", async ({
  page,
}) => {
  // Note -> note without passing through the tree: the context rail jumps directly
  // from one note to another (`open_note_by_id`), which both the rail and the chapter
  // editor's "Notes here" strip use. It is the sharpest case, because the editor model
  // is shared — so this is also where a careless flush would write the outgoing note's
  // text under the *incoming* note's id. Both halves are asserted.
  await registerNewUser(page);
  const bookId = await createBook(page, "Rail Exit");
  await createNote(page, "Vess");
  await createNote(page, "The siege");

  // Seed the outbound edge server-side so the rail has a row to click, without
  // depending on the sigil menu (that is `notes-sigils.spec.ts`'s job).
  const notes = await (await page.request.get(`/api/books/${bookId}/notes`)).json();
  const siege = notes.notes.find((n: { title: string }) => n.title === "The siege");
  await page.request.put(`/api/books/${bookId}/notes/${siege.id}`, {
    data: {
      content: JSON.stringify({
        type: "doc",
        content: [{ type: "paragraph", content: [{ type: "text", text: "@Vess " }] }],
      }),
    },
  });

  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await openNoteFromTree(page, "The siege");

  const writes = watchNoteWrites(page);
  const text = "Typed, then abandoned by following the rail.";
  await typeInNote(page, text);
  expect(writes, "the 800ms debounce must still be pending").toEqual([]);

  // Follow the outbound link to Vess.
  await page.locator(".note-rail-group.is-out .note-rail-row").first().click();
  await expect(page.locator(".note-editor-topbar-left")).toContainText("Vess");

  expect(writes, "following a rail link must write the note being left").not.toEqual([]);
  await expectNoteContains(page, bookId, "The siege", text);

  // And Vess was not overwritten with the siege's prose on the way past — the
  // crosstalk the shared editor model makes possible.
  await openNotesPane(page);
  await openNoteFromTree(page, "Vess");
  await expect(page.locator(NOTE_EDITOR)).not.toContainText("Typed, then abandoned");
});

test("leaving the book for the dashboard saves the note", async ({ page }) => {
  const text = "Note text that must survive walking out to the dashboard.";
  const { bookId, writes } = await typeIntoFreshNote(page, "Dashboard Exit", text);

  await page.locator('.ws-tools .tool[data-tip="All books"]').click();
  await expect(page).toHaveURL(/\/$/);

  expect(writes, "leaving for the dashboard must write the note").not.toEqual([]);
  await expectNoteContains(page, bookId, "The siege", text);
});

test("a tools-strip pane saves the note it is leaving", async ({ page }) => {
  // Typography stands in for the whole footer strip (Typography / Beta readers /
  // History / Preview) — they share one handler shape.
  const text = "Note text that must survive a detour through Typography.";
  const { bookId, writes } = await typeIntoFreshNote(page, "Typography Exit", text);

  await page.locator('.ws-tools .tool[data-tip="Typography"]').click();

  expect(writes, "opening Typography must write the note").not.toEqual([]);
  await expectNoteContains(page, bookId, "The siege", text);
});

test("a note merely opened and left is never written", async ({ page }) => {
  // The other half of the rule, and the one that cost a paragraph in production: the
  // flush must not turn every walk-past into a save. No edit, no write — see
  // `no-save-without-edit.spec.ts` for the chapter side of the same guard.
  await registerNewUser(page);
  await createBook(page, "Looked At Only");
  await addChapter(page, "Somewhere Else");
  await createNote(page, "The siege");
  await createNote(page, "Vess");

  await openNotesPane(page);
  await openNoteFromTree(page, "The siege");
  await page.waitForTimeout(1500);

  // From here on, any PUT to a note is a write nobody asked for.
  const writes = watchNoteWrites(page);

  await openNotesPane(page);
  await openNoteFromTree(page, "Vess");
  await page.waitForTimeout(1500);
  await page.locator('.ws-tools .tool[data-tip="Typography"]').click();
  await page.waitForTimeout(1500);

  expect(writes, `no note should be written when nothing was edited: ${writes}`).toEqual([]);
});
