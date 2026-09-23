import { Page, expect } from "@playwright/test";

/** A unique-ish username so tests don't collide on the shared server DB. */
export function uniqueUser(prefix = "e2e"): string {
  const rand = Math.random().toString(36).slice(2, 8);
  return `${prefix}_${Date.now().toString(36)}_${rand}`;
}

/** Register a brand-new account through the UI and land on the dashboard. */
export async function registerNewUser(
  page: Page,
  username = uniqueUser(),
  password = "password123",
): Promise<{ username: string; password: string }> {
  // Guarantee a clean, unauthenticated state — otherwise /register redirects
  // straight to the dashboard for an already-logged-in session.
  await page.context().clearCookies();
  await page.goto("/register");
  await page.locator("input[placeholder='Choose a username']").waitFor();
  await page.locator("input[placeholder='Choose a username']").fill(username);
  await page.locator("input[placeholder='your@email.com']").fill(`${username}@example.com`);
  await page.locator("input[placeholder='Choose a password']").fill(password);
  await page.locator("input[placeholder='Repeat your password']").fill(password);
  await page.getByRole("button", { name: "Create account" }).click();

  // Landed on the dashboard (root), which shows the username.
  await expect(page).toHaveURL(/\/$|\/$/);
  await expect(page.getByText(username, { exact: false }).first()).toBeVisible();
  return { username, password };
}

/** Log out via the dashboard topbar icon and land back on /login. */
export async function logout(page: Page) {
  await page.goto("/");
  // The topbar's icons are dark-mode toggle, settings, logout — addressed by
  // class rather than position so adding an icon doesn't silently retarget this.
  await page.locator(".dash-topbar-right .dash-logout").click();
  await expect(page).toHaveURL(/\/login/);
}

/** Log in through the UI with existing credentials. */
export async function login(page: Page, username: string, password: string) {
  await page.goto("/login");
  await page.locator("input[placeholder='Your username']").fill(username);
  await page.locator("input[placeholder='Your password']").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
}

/** Create a book from the dashboard and return its id (from the URL once open). */
export async function createBook(page: Page, title: string): Promise<string> {
  await page.getByRole("button", { name: "New Book" }).first().click();
  await page.locator("input[placeholder='Book title']").fill(title);
  // The visible "Create" button inside the new-book modal.
  await page.locator(".rinch-modal__body:visible").getByRole("button", { name: "Create" }).click();

  // Open the freshly created book card.
  await page.getByText(title, { exact: true }).first().click();
  await expect(page).toHaveURL(/\/book\/[0-9a-f-]{36}/);
  const url = page.url();
  return url.split("/book/")[1];
}

/**
 * Add a chapter via the chapters pane's inline "Add chapter" row.
 *
 * Post-overlay-tiers (design/01-language.html#overlays, Tier 1), this no
 * longer opens a dialog: clicking "Add chapter" appends an empty draft row
 * (`#chapter-inline-title`, focused) directly in the chapter list, and Enter
 * commits it (see `focus_inline_input` / `commit_new_chapter` in
 * pages/book/panes/chapters.rs).
 */
export async function addChapter(page: Page, title: string) {
  await page.getByRole("button", { name: "Add chapter" }).first().click();
  const input = page.locator("#chapter-inline-title");
  // The Enter/Escape/blur listeners are wired from a `set_timeout(0, ..)`
  // inside an effect (`focus_inline_input`, pages/book/panes/chapters.rs) —
  // they attach a tick after the input mounts, so a `.press("Enter")` fired
  // immediately after `.fill()` can race ahead of them and land on an input
  // with no commit handler yet. Waiting for the input to actually be focused
  // (the synchronous part of that same function, called before the listeners
  // are wired) is a reasonably tight proxy for "the effect has run".
  await input.waitFor({ state: "visible" });
  await expect(input).toBeFocused();
  await input.fill(title);
  await input.press("Enter");
  // The chapter shows up as a row in the chapters pane.
  await expect(
    page.locator(".chapter-rows .crow .t", { hasText: title }),
  ).toBeVisible();
}

/**
 * Open the Notes pane from the book sidebar.
 *
 * The sidebar "Notes" section header (`.pw-section-header`, renamed from
 * `.sidebar-section-header` when the sidebar became rows — see
 * `components/section_header.rs`) toggles the notes pane into the main
 * content area (a CSS `display` toggle). Clicking it flips `active_pane` to
 * Notes; we wait for the pane header to be visible before returning.
 */
export async function openNotesPane(page: Page) {
  await page
    .locator(".pw-section-header", { hasText: "Notes" })
    .getByText("Notes", { exact: true })
    .click();
  await expect(page.locator(".notes-pane-header")).toBeVisible();
}

/**
 * Create a note via the "Add Note" modal (mirrors `addChapter`).
 *
 * Opens the notes pane first (the "Add Note" button lives inside it), fills the
 * title, confirms, and waits for the new note card to appear in the tree.
 */
export async function createNote(page: Page, title: string) {
  await openNotesPane(page);
  await page.getByRole("button", { name: "Add Note" }).first().click();
  const modal = page.locator(".rinch-modal__body:visible");
  await modal.locator("input[placeholder='Enter note title']").fill(title);
  await modal.getByRole("button", { name: "Add", exact: true }).click();
  // The note shows up as a card in the tree.
  await expect(
    page.locator(".notes-tree .note-card-title", { hasText: title }),
  ).toBeVisible();
}

/**
 * Open the Beta Readers pane from the book sidebar.
 *
 * No longer a sidebar section header: Typography / Beta Readers / History
 * demoted to the footer tools strip (`.ws-tools .tool`, one icon each,
 * `data-tip="Beta readers"`) when the sidebar IA was reworked. Clicking it
 * flips `active_pane` to `BookPane::BetaReaders`, revealing the pane's
 * "Beta Readers" heading.
 */
export async function openBetaReadersPane(page: Page) {
  await page.locator('.ws-tools .tool[data-tip="Beta readers"]').click();
  await expect(page.getByRole("heading", { name: "Beta Readers" })).toBeVisible();
}

/**
 * Create a beta reader link through the Beta Readers pane UI and return its
 * server-side `token`.
 *
 * Opens the pane, clicks "Create Link", fills the reader name in the Sheet
 * (a right-hand slide-over — Tier 3 of the overlay redesign, `.pw-sheet-body`,
 * not `.rinch-modal__body`; see `components/sheet.rs`), and confirms. The
 * link's token isn't surfaced in the DOM, so we read it back from
 * `GET /api/books/{id}/beta-links` (polled until the just-created link appears,
 * proving the POST landed server-side).
 */
export async function createBetaLink(
  page: Page,
  bookId: string,
  readerName: string,
): Promise<string> {
  await openBetaReadersPane(page);
  await page.getByRole("button", { name: "Create Link" }).click();
  // Both the Create and Edit beta-link Sheets stay mounted while closed
  // (rinch #761 — a Drawer never unmounts, only slides off/hides), so
  // `.pw-sheet-body` always matches two; scope to the one that's visible.
  const sheet = page.locator(".pw-sheet-body:visible");
  await sheet
    .locator("input[placeholder='e.g. Alice, Book Club, etc.']")
    .fill(readerName);
  await sheet.getByRole("button", { name: "Create", exact: true }).click();
  // The sheet closes once the POST succeeds.
  await expect(page.locator(".pw-sheet-body:visible")).toHaveCount(0);

  let token = "";
  await expect
    .poll(async () => {
      const resp = await page.request.get(`/api/books/${bookId}/beta-links`);
      const links = (await resp.json()) as Array<{
        token: string;
        reader_name: string;
      }>;
      token = links.find((l) => l.reader_name === readerName)?.token ?? "";
      return token;
    })
    .not.toBe("");
  return token;
}

/**
 * Create a chapter and stuff it with enough prose to span many paginated pages
 * in the reader at the default 1280x720 viewport.
 *
 * The chapter is created through the UI (so it lands in the sidebar / book.json
 * exactly like a real one), then its content is written directly via the
 * authenticated chapter PUT endpoint — far more deterministic than typing a wall
 * of text into the contenteditable editor. The reader renders markdown where
 * every non-empty line becomes its own `<p>`, so a big pile of paragraph lines
 * reliably overflows a single column into multiple pages.
 *
 * Returns the new chapter's server id.
 */
export async function seedLongChapter(
  page: Page,
  bookId: string,
  title: string,
  paragraphs = 80,
): Promise<string> {
  await addChapter(page, title);

  // Resolve the freshly created chapter's id from the authenticated list.
  const listResp = await page.request.get(`/api/books/${bookId}/chapters`);
  const chapters = (await listResp.json()) as Array<{ id: string; title: string }>;
  const chapterId = chapters.find((c) => c.title === title)?.id;
  if (!chapterId) throw new Error(`chapter not found after create: ${title}`);

  // A deterministic block of prose: `paragraphs` distinct lines, each its own
  // `<p>` once rendered. Distinct text per line avoids any de-duping surprises.
  const lorem =
    "The lantern guttered against the fog while the harbour bell counted out the hours and the tide dragged its slow grey fingers across the shingle below the cliff path.";
  const body = Array.from(
    { length: paragraphs },
    (_, i) => `Paragraph ${i + 1}. ${lorem}`,
  ).join("\n\n");

  const putResp = await page.request.put(
    `/api/books/${bookId}/chapters/${chapterId}`,
    { data: { content: body } },
  );
  if (!putResp.ok()) {
    throw new Error(`failed to seed chapter content: ${putResp.status()}`);
  }
  return chapterId;
}

/**
 * Open the in-editor feedback sidebar. The toggle (a MessageCircle ActionIcon in
 * the editor topbar) only renders once the book has at least one feedback item,
 * so a chapter/editor must already be open and feedback must exist. Clicking it
 * flips `.editor-feedback-sidebar` from `.hidden` to `.visible`.
 */
export async function openFeedbackSidebar(page: Page) {
  // The editor topbar has two ActionIcons: back-arrow (first) and the feedback
  // toggle (last, only present when feedback exists).
  await page.locator(".editor-topbar .rinch-action-icon").last().click();
  await expect(page.locator(".editor-feedback-sidebar.visible")).toBeVisible();
}

/**
 * Open a chapter in the editor by its sidebar name; waits until it's ready.
 *
 * The prose editor is the model-first `rinch-editor-view` (not a
 * `contenteditable`): it mounts a `[data-pm-editor]` surface inside `#editor-main`,
 * and the whole editor pane (`.editor-layout`) is `display:none` until a chapter
 * pane is active — so the surface becoming *visible* is the "chapter loaded" signal.
 */
export async function openChapter(page: Page, title: string) {
  const item = page.locator(".sidebar-chapter-item", { hasText: title });
  await expect(item).toBeVisible();
  await item.click();
  // Wait for the chapter's editor pane to become active (visible).
  await page
    .locator("#editor-main [data-pm-editor]")
    .waitFor({ state: "visible", timeout: 15_000 });
}

/**
 * Type prose into the model-first editor. It is deliberately **not** a
 * `contenteditable`, so Playwright's `.fill()` (which needs a native editable)
 * does nothing — focus the surface with a real click and send real keystrokes,
 * which the editor's global key handler turns into document edits.
 */
export async function typeInEditor(page: Page, text: string) {
  const surface = page.locator("#editor-main [data-pm-editor]");
  await surface.waitFor({ state: "visible", timeout: 15_000 });
  await surface.click();
  await page.keyboard.type(text);
}

/**
 * Explicitly return the collapsed writing-mode chrome (sidebar, topbar,
 * toolbar) before interacting with it.
 *
 * `.book-workspace.is-writing` / `.editor-layout.is-writing` collapse the
 * sidebar to zero width and fade the topbar/toolbar while typing, and — since
 * the idle-return timer was removed — nothing brings them back on its own; the
 * app returns them only on a real pointer move or Escape (see
 * `start_writing`/`stop_writing` in `pages/book/mod.rs`). A real author
 * reaches for the sidebar by moving the mouse toward it, which fires the
 * window `mousemove` listener and restores the chrome before the click ever
 * lands. Playwright's synthetic `.click()` can't do that implicitly: its
 * actionability check refuses to move the pointer at all once it sees the
 * target sits under a `pointer-events: none` ancestor, so clicking a
 * still-collapsed sidebar/toolbar control right after `typeInEditor` would
 * retry until the test times out. Call this first — it is the same
 * "bring it back" affordance the app offers the author directly.
 */
export async function returnChrome(page: Page) {
  await page.keyboard.press("Escape");
  await expect(page.locator(".book-workspace")).not.toHaveClass(/is-writing/);
}

/** Open Settings from the dashboard topbar's gear icon. */
export async function openSettings(page: Page) {
  await page.locator(".dash-topbar-right .dash-settings").click();
  await expect(page).toHaveURL(/\/settings$/);
  await expect(page.getByRole("heading", { name: "Agent access" })).toBeVisible();
}
