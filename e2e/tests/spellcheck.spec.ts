import { test, expect, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, returnChrome, typeInEditor } from "./helpers";

/**
 * The squiggle is a decoration, not document content: rinch's editor view wraps
 * a decorated run in `<span data-pm-deco class="…">`, and the spellcheck plugin
 * asks for the class `pm-spell-error` (see `plotweb-web/src/spell/plugin.rs`).
 * So this locator is the whole contract between the plugin and the screen.
 */
const squiggles = (page: Page) =>
  page.locator("#editor-main [data-pm-deco].pm-spell-error");

const squiggle = (page: Page, word: string) => squiggles(page).filter({ hasText: word });

/**
 * The dictionary is ~860 KB fetched from `/api/dictionaries/en_US.*` and parsed
 * in wasm on first use, so the *first* squiggle of a run can be several seconds
 * behind the keystroke that earned it. Later ones come off the cached copy.
 */
const FIRST_SQUIGGLE_TIMEOUT = 25_000;

/**
 * Move the caret out of whatever word it is sitting in.
 *
 * The plugin withholds the squiggle on the word under a collapsed caret — a word
 * being typed is not yet a misspelling — and a right-click *places* the caret in
 * the word it opens the menu on. So after any menu action, the caret has to leave
 * before "is it still underlined?" means anything.
 */
async function caretToEndOfLine(page: Page) {
  await page.keyboard.press("End");
}

test("a misspelling is underlined, and 'Add to dictionary' clears it for good", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Spellcheck Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  // Two typos, so "the squiggle went away" can be told apart from "squiggles
  // stopped working". The trailing space matters: it moves the caret off the
  // last word, which would otherwise be treated as still being typed.
  await typeInEditor(page, "She recieved the letter adn waited ");

  await expect(squiggle(page, "recieved")).toBeVisible({ timeout: FIRST_SQUIGGLE_TIMEOUT });
  await expect(squiggles(page)).toHaveCount(2);

  // Right-click it. Both rinch backends move the caret to a context press before
  // the app's `oncontextmenu` runs, which is how the menu knows which word it is.
  await squiggle(page, "recieved").click({ button: "right" });
  const menu = page.locator(".spell-menu");
  await expect(menu).toBeVisible();
  await expect(menu.locator(".spell-menu-suggestion", { hasText: "received" })).toBeVisible();

  await menu.getByText("Add to dictionary").click();
  await expect(menu).toHaveCount(0);

  await caretToEndOfLine(page);
  await expect(squiggle(page, "recieved")).toHaveCount(0);
  await expect(squiggle(page, "adn")).toBeVisible();

  // The word reached the account, not just this tab.
  await expect
    .poll(async () => {
      const resp = await page.request.get("/api/me/dictionary");
      const body = (await resp.json()) as { words: string[] };
      return body.words;
    })
    .toContain("recieved");

  // Past the autosave debounce so the prose itself is on the server too.
  await page.waitForTimeout(4000);

  // A full reload: a new speller, a new local dictionary read, the same verdict.
  await page.goto(`/book/${bookId}`);
  await openChapter(page, "Chapter One");
  await expect(page.locator("#editor-main")).toContainText("She recieved the letter adn waited");
  // `adn` coming back is what proves the checker ran at all on this load — so a
  // silent failure to load the dictionary cannot pass as "no misspellings".
  await expect(squiggle(page, "adn")).toBeVisible({ timeout: FIRST_SQUIGGLE_TIMEOUT });
  await expect(squiggle(page, "recieved")).toHaveCount(0);
});

test("a suggestion replaces the word, and 'Ignore' only lasts the session", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Suggestion Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "They seperate the herds ");
  await expect(squiggle(page, "seperate")).toBeVisible({ timeout: FIRST_SQUIGGLE_TIMEOUT });

  await squiggle(page, "seperate").click({ button: "right" });
  await page.locator(".spell-menu-suggestion", { hasText: "separate" }).first().click();
  await expect(page.locator(".spell-menu")).toHaveCount(0);
  await expect(page.locator("#editor-main")).toContainText("They separate the herds");
  await caretToEndOfLine(page);
  await expect(squiggles(page)).toHaveCount(0);

  // Now a word no dictionary will ever hold, dismissed with "Ignore".
  await typeInEditor(page, "and Vashkeep burned ");
  await expect(squiggle(page, "Vashkeep")).toBeVisible({ timeout: FIRST_SQUIGGLE_TIMEOUT });
  await squiggle(page, "Vashkeep").click({ button: "right" });
  await page.locator(".spell-menu").getByText("Ignore").click();
  await caretToEndOfLine(page);
  await expect(squiggle(page, "Vashkeep")).toHaveCount(0);

  // Session-only: it is not pushed to the account…
  const resp = await page.request.get("/api/me/dictionary");
  const body = (await resp.json()) as { words: string[] };
  expect(body.words).not.toContain("Vashkeep");

  // …and a reload underlines it again.
  await page.waitForTimeout(4000);
  await page.goto(`/book/${bookId}`);
  await openChapter(page, "Chapter One");
  await expect(squiggle(page, "Vashkeep")).toBeVisible({ timeout: FIRST_SQUIGGLE_TIMEOUT });
});

test("the Typography switch turns the squiggles off on this device", async ({ page }) => {
  await registerNewUser(page);
  await createBook(page, "Switch Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "A recieved letter ");
  await expect(squiggle(page, "recieved")).toBeVisible({ timeout: FIRST_SQUIGGLE_TIMEOUT });

  // Typography lives in the footer tools strip, not the sidebar — which is
  // collapsed to zero width right now (`is-writing`, no idle timer to bring it
  // back on its own), so ask for the chrome back first.
  await returnChrome(page);
  await page.locator('.ws-tools .tool[data-tip="Typography"]').click();
  await page.locator("#pw-spellcheck .rinch-switch").click();

  await openChapter(page, "Chapter One");
  await expect(squiggles(page)).toHaveCount(0);

  // Back on, and it returns.
  await page.locator('.ws-tools .tool[data-tip="Typography"]').click();
  await page.locator("#pw-spellcheck .rinch-switch").click();
  await openChapter(page, "Chapter One");
  await expect(squiggle(page, "recieved")).toBeVisible({ timeout: FIRST_SQUIGGLE_TIMEOUT });
});
