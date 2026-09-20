import { test, expect, Browser, Page } from "@playwright/test";
import {
  addChapter,
  createBook,
  openChapter,
  registerNewUser,
  returnChrome,
  typeInEditor,
} from "./helpers";

/**
 * What the save indicator is allowed to claim in the configuration production runs:
 * a cut-over book, sync on.
 *
 * There the sync engine is the *only* writer of a chapter body — the REST PUT
 * deliberately withholds `content` — so every keystroke has to pass through rinch's
 * CRDT projection. Anything that projection refuses stays in this tab, and refuses
 * *silently*: the PUT still succeeds, still returns durable, and the footer used to
 * say "Saved" over a document the server had never seen. An author could write for an
 * hour into one browser tab with the indicator agreeing the whole way.
 *
 * Two halves, one spec:
 *
 * - **hard_break** (Shift+Enter) used to be one of those refusals, so in a cut-over
 *   book *nothing* reached the server from the first Shift+Enter onward. rinch PR #838
 *   brought inline atoms into scope; this half proves the whole body now round-trips.
 * - **blockquote** is still out of scope and cannot be brought in from here. This half
 *   proves the remaining failure mode is at least honest — the footer says
 *   "Unsaved — unsupported content" and never "Saved" — and that removing the
 *   offending block resumes normal saving.
 */

// Needs a server started with `PLOTWEB_CUTOVER_BOOKS=*`; run with
// `PLOTWEB_E2E_CUTOVER=* npx playwright test cutover-inline-atoms`. Against the default
// server these would exercise the git-authoritative paths, where REST carries the body
// and neither claim means anything.
test.skip(
  !process.env.PLOTWEB_E2E_CUTOVER,
  "requires a cut-over server — run `npm run test:cutover`",
);

/** A browser context with sync switched on, as `cutover-sync.spec.ts` does. */
async function openDevice(browser: Browser, baseURL: string): Promise<Page> {
  const context = await browser.newContext();
  await context.addInitScript(() => {
    window.localStorage.setItem("plotweb_sync", "1");
  });
  const page = await context.newPage();
  await page.goto(baseURL);
  return page;
}

const saveIndicator = (page: Page) => page.locator(".save-indicator");

/**
 * The blockquote toolbar button, targeted by its Tabler icon's path data the way
 * `scene-break.spec.ts` targets the scene break — the buttons carry no name, role or
 * tooltip hook. `TablerIcon::Blockquote`'s curled-quote stroke is unique across the
 * whole icon set, so this cannot collide with another toolbar control.
 */
const blockquoteButton = (page: Page) =>
  page.locator(
    '.editor-layout .toolbar .rinch-action-icon:has(path[d="M9 9h1a1 1 0 1 1 -1 1v-2.5a2 2 0 0 1 2 -2"])',
  );

test("a hard break syncs, so the whole chapter survives a reload", async ({
  browser,
  baseURL,
}) => {
  // Before rinch #838 this was the sharp edge: the Shift+Enter itself failed to
  // project, outbound stalled, and *every* edit after it — including the second
  // sentence — stayed in the tab. The reload is what exposes it, because a cut-over
  // book reads its body back from the canonical document sync was supposed to write.
  test.setTimeout(120_000);

  const first = "The first line ends with a deliberate break.";
  const second = "The second line shares its paragraph with the first.";

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Hard Break Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, first);
  // rinch's default keymap maps Shift-Enter to `insertHardBreak`: an inline atom
  // inside the *same* paragraph, not a new block.
  await page.keyboard.press("Shift+Enter");
  await page.keyboard.type(second);

  // The footer is the claim under test, so wait for it to make the claim rather than
  // sleeping past the 3s debounce. Sync is on, so it is plain "Saved" — not the
  // "Saved on this device" wording a cut-over book shows with sync off.
  await expect(saveIndicator(page)).toHaveText("Saved", { timeout: 20_000 });
  // And give the sync round itself time to land before the reload takes the tab's
  // in-memory copy away with it.
  await page.waitForTimeout(8000);

  await page.reload();
  await openChapter(page, "Chapter One");

  // One paragraph, holding both sentences with a `br` between them — that is what a
  // `hard_break` renders as (its schema tag is `br`), and requiring the text as well
  // as the tag keeps this from being satisfied by an empty-paragraph filler `br`.
  const brokenParagraph = page.locator("#editor-main [data-pm-editor] p", {
    has: page.locator("br"),
  });
  await expect(brokenParagraph).toContainText(first, { timeout: 15_000 });
  await expect(brokenParagraph).toContainText(second);
});

test("a blockquote says it is not saving, and saves again once it is removed", async ({
  browser,
  baseURL,
}) => {
  // Blockquote is still outside the collab projection's scope, so this edit genuinely
  // does not reach the server. The bug was never that — it was the footer saying
  // "Saved" about it.
  test.setTimeout(120_000);

  const page = await openDevice(browser, baseURL!);
  await registerNewUser(page);
  await createBook(page, "Blockquote Honesty Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "A line that is about to become a quotation.");
  // Normal prose still saves normally — this is the baseline the stall has to break
  // away from, so that reaching "Saved" later proves recovery rather than inertia.
  await expect(saveIndicator(page)).toHaveText("Saved", { timeout: 20_000 });

  // Typing collapsed the chrome; the toolbar is `pointer-events: none` until it comes
  // back, so Playwright would retry the click until the test timed out.
  await returnChrome(page);
  await expect(blockquoteButton(page)).toBeVisible();
  await blockquoteButton(page).click();
  await expect(page.locator("#editor-main [data-pm-editor] blockquote")).toHaveCount(1);

  // The indicator flips as soon as the edit is refused — not at the end of the 3s
  // autosave debounce.
  await expect(saveIndicator(page)).toHaveText("Unsaved — unsupported content", {
    timeout: 10_000,
  });

  // And it stays that way well past the debounce. This is the whole point: the REST
  // round trip *does* complete, durably, having carried no body at all, and that
  // receipt must not be allowed to write "Saved" over this.
  const until = Date.now() + 6000;
  while (Date.now() < until) {
    await expect(saveIndicator(page)).toHaveText("Unsaved — unsupported content");
    await page.waitForTimeout(500);
  }

  // Remove the blockquote. rinch's wrap is a real, invertible transaction, so undo
  // lifts it; more than one step may be on the history stack (the typing coalesces
  // separately from the wrap), so press until the quote is gone.
  await page.locator("#editor-main [data-pm-editor] blockquote p").last().click();
  for (let i = 0; i < 4; i++) {
    if ((await page.locator("#editor-main [data-pm-editor] blockquote").count()) === 0) {
      break;
    }
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(300);
  }
  await expect(page.locator("#editor-main [data-pm-editor] blockquote")).toHaveCount(0);

  // The stall clears on the first edit that projects again, and rinch broadcasts
  // everything that accumulated meanwhile — so the next save is a real one.
  await expect(saveIndicator(page)).toHaveText("Saved", { timeout: 20_000 });
});
