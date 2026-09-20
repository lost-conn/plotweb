import { test, expect, Locator, Page } from "@playwright/test";
import {
  addChapter,
  createBook,
  openChapter,
  registerNewUser,
} from "./helpers";

/**
 * Book-wide find and replace.
 *
 * Three things are being pinned here, and only the first is really about the
 * matcher (which has its own unit tests in `plotweb-web/src/find/matcher.rs`):
 *
 *  1. The panel counts the *whole book* from `AppStore.chapters`' stored bodies
 *     while only one chapter is open, and the Match case / Whole word switches
 *     change those counts.
 *  2. Clicking a hit in a chapter that is **not** open switches to it and lands
 *     on the word — which is the hit-*index* round trip (`panes/find.rs`:
 *     a position found in a stored copy is never trusted against the live
 *     document, the chapter is re-searched once it is open).
 *  3. "Replace all in book" walks every chapter with hits, waiting for each
 *     one's CRDT body to attach before it edits — the constraint the whole
 *     feature is shaped by (see `find/mod.rs`). A reload then has to still show
 *     the replacement, which is the only proof the walk actually *saved*.
 */

/** The panel, and the controls inside it. */
const panel = (page: Page): Locator => page.locator(".find-panel");
const queryField = (page: Page): Locator => page.locator("#find-query");
const replaceField = (page: Page): Locator => page.locator("#find-replace");
const countLabel = (page: Page): Locator => panel(page).locator(".find-count");

/** One chapter's result group, by the chapter title in its header. */
const group = (page: Page, title: string): Locator =>
  panel(page).locator(".find-group").filter({
    has: page.locator(".find-group-title", { hasText: title }),
  });

/** The number a group's header is reporting. */
async function groupCount(page: Page, title: string): Promise<number> {
  const text = await group(page, title).locator(".find-group-count").innerText();
  return Number(text.trim());
}

/** Every hit row in the panel, across all chapters. */
const hitRows = (page: Page): Locator => panel(page).locator(".find-hit");

/**
 * The decoration spans the highlighter asks for. Same contract as the
 * spellchecker's `pm-spell-error`: rinch's editor view wraps a decorated run in
 * `<span data-pm-deco class="…">` and the plugin does nothing but name a class
 * (`plotweb-web/src/find/plugin.rs`).
 */
const hitSpans = (page: Page): Locator =>
  page.locator("#editor-main [data-pm-deco].pm-search-hit");
const currentHitSpan = (page: Page): Locator =>
  page.locator("#editor-main [data-pm-deco].pm-search-hit-current");

/**
 * A toggle in the options row, by its title attribute — the buttons are two
 * glyphs each ("Aa", "Ab|"), so the tooltip is the only readable handle.
 */
const toggleButton = (page: Page, tooltip: string): Locator =>
  panel(page).locator(`.find-toggle[title="${tooltip}"]`);

/**
 * Create a chapter through the UI and give it exactly this prose, in **one**
 * body PUT.
 *
 * `seedLongChapter` would be the obvious helper, but it writes its own lorem
 * and this test needs exact counts — and overwriting it afterwards is not an
 * option: under cut-over the *second* body PUT for a chapter is refused
 * (`deferred_to_sync`), because by then the canonical document exists and sync
 * owns the body. The first write still lands in both git and the canonical
 * copy, so one PUT it is. (That rule is the whole reason replace-all goes
 * through a live editor rather than the API — see `find/mod.rs`.)
 */
async function seedChapter(
  page: Page,
  bookId: string,
  title: string,
  paragraphs: string[],
): Promise<string> {
  await addChapter(page, title);
  const listResp = await page.request.get(`/api/books/${bookId}/chapters`);
  const chapters = (await listResp.json()) as Array<{ id: string; title: string }>;
  const chapterId = chapters.find((c) => c.title === title)?.id;
  if (!chapterId) throw new Error(`chapter not found after create: ${title}`);

  const resp = await page.request.put(
    `/api/books/${bookId}/chapters/${chapterId}`,
    { data: { content: paragraphs.join("\n\n") } },
  );
  expect(resp.ok()).toBeTruthy();
  return chapterId;
}

/** Open the find panel over the whole book. */
async function openFindPanel(page: Page) {
  // The shortcut is a window `keydown` listener (`pages/book/mod.rs`). Nothing
  // has to be focused first: rinch's editor consumes Ctrl-modified keys it has
  // no binding for *not at all* (`editor_input.rs`, "Ctrl/Alt combos stay
  // UNCONSUMED"), so the key reaches the window from wherever focus is.
  await page.keyboard.press("Control+Shift+F");
  await expect(panel(page)).toBeVisible();
}

/**
 * The chapters used by the counting tests. `lantern` appears:
 *   - Chapter One:   3 times (one capitalised, one inside `lanterns`)
 *   - Chapter Two:   2 times (both lowercase, both standalone)
 *   - Chapter Three: 1 time  (capitalised, standalone)
 *
 * So the whole-book count is 6; Match case (searching "lantern") drops the two
 * capitalised ones to 4; Whole word drops the one inside `lanterns` to 5.
 */
const CHAPTERS: Array<{ title: string; paragraphs: string[] }> = [
  {
    title: "Chapter One",
    paragraphs: [
      "The lantern guttered against the fog.",
      "Lantern light fell across the shingle, and the lanterns beyond it went out.",
    ],
  },
  {
    title: "Chapter Two",
    paragraphs: [
      "She carried the lantern down to the water.",
      "A lantern is a small argument against the dark.",
    ],
  },
  {
    title: "Chapter Three",
    paragraphs: ["Lantern smoke hung under the cliff path."],
  },
];

async function seedBook(page: Page, bookTitle: string): Promise<string> {
  await registerNewUser(page);
  const bookId = await createBook(page, bookTitle);
  for (const chapter of CHAPTERS) {
    await seedChapter(page, bookId, chapter.title, chapter.paragraphs);
  }
  // Re-enter the book so `AppStore.chapters` is loaded with the seeded bodies —
  // the whole-book search reads those, and the copy this page fetched at create
  // time predates every PUT above.
  await page.goto(`/book/${bookId}`);
  await expect(
    page.locator(".sidebar-chapter-item", { hasText: "Chapter Three" }),
  ).toBeVisible();
  return bookId;
}

test("Ctrl+Shift+F counts every chapter, and the switches change the counts", async ({
  page,
}) => {
  await seedBook(page, "Find Novel");
  await openChapter(page, "Chapter One");
  await openFindPanel(page);

  await queryField(page).fill("lantern");

  // Grouped by chapter, with a count each. Chapter One is the open one, so it
  // is counted from the live editor document; the other two from their stored
  // bodies — and the two paths have to agree about what a match is.
  await expect.poll(async () => groupCount(page, "Chapter One")).toBe(3);
  expect(await groupCount(page, "Chapter Two")).toBe(2);
  expect(await groupCount(page, "Chapter Three")).toBe(1);
  await expect(hitRows(page)).toHaveCount(6);
  await expect(countLabel(page)).toHaveText("6 matches");

  // The open chapter's hits are painted into the prose.
  await expect(hitSpans(page)).toHaveCount(3);

  // ── Match case ────────────────────────────────────────────────────
  // "lantern" lowercase: the two capitalised occurrences drop out (one in
  // Chapter One, the whole of Chapter Three).
  await toggleButton(page, "Match case").click();
  await expect.poll(async () => hitRows(page).count()).toBe(4);
  expect(await groupCount(page, "Chapter One")).toBe(2);
  expect(await groupCount(page, "Chapter Two")).toBe(2);
  // A chapter with no hits is not listed at all.
  await expect(group(page, "Chapter Three")).toHaveCount(0);
  await toggleButton(page, "Match case").click();
  await expect.poll(async () => hitRows(page).count()).toBe(6);

  // ── Whole word ────────────────────────────────────────────────────
  // `lanterns` stops counting.
  await toggleButton(page, "Whole word").click();
  await expect.poll(async () => hitRows(page).count()).toBe(5);
  expect(await groupCount(page, "Chapter One")).toBe(2);
  await toggleButton(page, "Whole word").click();
  await expect.poll(async () => hitRows(page).count()).toBe(6);

  // ── Escape closes and clears ──────────────────────────────────────
  await page.keyboard.press("Escape");
  await expect(panel(page)).toHaveCount(0);
  await expect(hitSpans(page)).toHaveCount(0);
});

test("clicking a hit in another chapter opens it and selects the word", async ({
  page,
}) => {
  await seedBook(page, "Find Navigation");
  await openChapter(page, "Chapter One");
  await openFindPanel(page);
  await queryField(page).fill("lantern");
  await expect.poll(async () => hitRows(page).count()).toBe(6);

  // The single hit in Chapter Three — a chapter that has never been opened, so
  // its hit was found in the stored body and has to be found again in the live
  // document once the chapter loads.
  await group(page, "Chapter Three").locator(".find-hit").first().click();

  // The editor is now showing Chapter Three...
  await expect(page.locator(".editor-title-input input")).toHaveValue(
    "Chapter Three",
  );
  await expect(
    page.locator("#editor-main [data-pm-editor]"),
  ).toContainText("cliff path");

  // ...its one hit is painted, and it is the current one.
  await expect(hitSpans(page)).toHaveCount(1);
  await expect(currentHitSpan(page)).toHaveCount(1);
  await expect(currentHitSpan(page)).toHaveText("Lantern");
  await expect(countLabel(page)).toHaveText("6 of 6");

  // Next wraps back round to the first hit in the first chapter, which means
  // switching chapters again — the same path, in the other direction.
  //
  // Enter only cycles hits while the caret is in one of the panel's own fields
  // (`find_field_focused`, `pages/book/mod.rs`) — an unscoped Enter would fire
  // while the author is typing a paragraph — and clicking a result row moved
  // focus to that row's button.
  await queryField(page).click();
  await page.keyboard.press("Enter");
  await expect(page.locator(".editor-title-input input")).toHaveValue(
    "Chapter One",
  );
  await expect(countLabel(page)).toHaveText("1 of 6");
  await expect(currentHitSpan(page)).toHaveCount(1);
});

test("Replace all in book rewrites every chapter and it survives a reload", async ({
  page,
}) => {
  const bookId = await seedBook(page, "Find Replace All");
  await openChapter(page, "Chapter One");
  await openFindPanel(page);

  await queryField(page).fill("lantern");
  await replaceField(page).fill("beacon");
  await expect.poll(async () => hitRows(page).count()).toBe(6);

  await panel(page).getByRole("button", { name: "Replace all in book" }).click();

  // Tier-2 confirm: it names the count before touching chapters the author is
  // not looking at.
  const dialog = page.locator(".rinch-modal__body:visible");
  await expect(dialog).toContainText("6 matches in 3 chapters");
  await dialog.getByRole("button", { name: "Replace all" }).click();

  // The walk re-runs the search when it finishes, and the honest answer then is
  // zero. Generous, because it opens each chapter and waits for its body doc.
  await expect
    .poll(async () => hitRows(page).count(), { timeout: 60_000 })
    .toBe(0);
  await expect(countLabel(page)).toHaveText("No matches");

  // Back where the author started.
  await expect(page.locator(".editor-title-input input")).toHaveValue(
    "Chapter One",
  );

  // ── The reload ────────────────────────────────────────────────────
  // Reading the editors back is the assertion that matters: under cut-over the
  // body travels by CRDT sync, so the REST mirror can lag, and this device's
  // own document is what a reopened chapter adopts.
  await page.goto(`/book/${bookId}`);
  for (const chapter of CHAPTERS) {
    await openChapter(page, chapter.title);
    const surface = page.locator("#editor-main [data-pm-editor]");
    await expect(surface).toContainText("beacon", { timeout: 15_000 });
    await expect(surface).not.toContainText(/lantern/i);
  }

  // Deliberately *not* asserted here: a fresh whole-book search over the
  // reloaded stored bodies. Under cut-over those come from git, which the
  // server's mirror only refreshes from the canonical copy on a 30s idle
  // debounce (`plotweb-server/src/mirror.rs`), so for half a minute after a
  // save the stored copy legitimately still says "lantern" while the canonical
  // one — the thing every reader and every editor load actually gets — says
  // "beacon". Waiting that out would buy a slow test and no extra coverage:
  // the loop above read all three chapters back through the live editor, which
  // is the copy that matters, and the panel re-searches a chapter's live
  // document the moment it is opened precisely because of this lag.
});

test("Replace and All in chapter act on the open chapter only", async ({
  page,
}) => {
  await seedBook(page, "Find Replace Chapter");
  await openChapter(page, "Chapter Two");
  await openFindPanel(page);

  await queryField(page).fill("lantern");
  await replaceField(page).fill("beacon");
  await expect.poll(async () => hitRows(page).count()).toBe(6);

  // Pick a hit *in the open chapter* explicitly. Next/Enter would not do:
  // under whole-book scope they cycle the book, so the first press would land
  // in Chapter One and switch away from the chapter this test is about.
  await group(page, "Chapter Two").locator(".find-hit").first().click();
  await expect(currentHitSpan(page)).toHaveCount(1);

  // Replace acts on that one hit, and only it.
  await panel(page).getByRole("button", { name: "Replace", exact: true }).click();
  await expect.poll(async () => hitRows(page).count()).toBe(5);
  expect(await groupCount(page, "Chapter Two")).toBe(1);

  // All in chapter clears the rest of this chapter and leaves the others alone.
  await panel(page).getByRole("button", { name: "All in chapter" }).click();
  await expect.poll(async () => hitRows(page).count()).toBe(4);
  await expect(group(page, "Chapter Two")).toHaveCount(0);
  expect(await groupCount(page, "Chapter One")).toBe(3);
  expect(await groupCount(page, "Chapter Three")).toBe(1);

  // One undo step for the whole All-in-chapter, not one per occurrence: the
  // replacements ride in a single transaction (`find/doc.rs`).
  await page.locator("#editor-main [data-pm-editor]").click();
  await page.keyboard.press("Control+z");
  await expect.poll(async () => hitRows(page).count()).toBe(5);
});
