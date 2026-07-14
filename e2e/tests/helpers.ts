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
  // The second action-icon in the topbar is the logout control (the first is
  // the dark-mode toggle).
  await page.locator(".dash-topbar-right .rinch-action-icon").nth(1).click();
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

/** Add a chapter via the "Add Chapter" modal. */
export async function addChapter(page: Page, title: string) {
  await page.getByRole("button", { name: "Add Chapter" }).first().click();
  const modal = page.locator(".rinch-modal__body:visible");
  await modal.locator("input[placeholder='Enter chapter title']").fill(title);
  await modal.getByRole("button", { name: "Add", exact: true }).click();
  // The chapter shows up in the sidebar.
  await expect(
    page.locator(".sidebar-chapter-name", { hasText: title }),
  ).toBeVisible();
}

/**
 * Open the Notes pane from the book sidebar.
 *
 * The sidebar "Notes" section header toggles the notes pane into the main
 * content area (a CSS `display` toggle). Clicking it flips `active_pane` to
 * Notes; we wait for the pane header to be visible before returning.
 */
export async function openNotesPane(page: Page) {
  await page
    .locator(".sidebar-section-header", { hasText: "Notes" })
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
 * Open the Beta Readers pane from the book sidebar (mirrors `openNotesPane`).
 *
 * The sidebar "Beta Readers" section header flips `active_pane` to
 * `BookPane::BetaReaders`, revealing the pane's "Beta Readers" heading.
 */
export async function openBetaReadersPane(page: Page) {
  await page
    .locator(".sidebar-section-header", { hasText: "Beta Readers" })
    .click();
  await expect(page.getByRole("heading", { name: "Beta Readers" })).toBeVisible();
}

/**
 * Create a beta reader link through the Beta Readers pane UI and return its
 * server-side `token`.
 *
 * Opens the pane, clicks "Create Link", fills the reader name in the modal, and
 * confirms. The link's token isn't surfaced in the DOM, so we read it back from
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
  const modal = page.locator(".rinch-modal__body:visible");
  await modal
    .locator("input[placeholder='e.g. Alice, Book Club, etc.']")
    .fill(readerName);
  await modal.getByRole("button", { name: "Create", exact: true }).click();
  // The modal closes once the POST succeeds.
  await expect(page.locator(".rinch-modal__body:visible")).toHaveCount(0);

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
