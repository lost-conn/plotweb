import { test, expect, Page } from "@playwright/test";
import {
  addChapter,
  createBook,
  createNote,
  openChapter,
  openNotesPane,
  registerNewUser,
  typeInEditor,
} from "./helpers";

/**
 * Sigil autocomplete + the context rail (notes revamp, card 2).
 *
 * Three things are being guarded here:
 *
 * 1. **The menu is anchored at the caret.** The note editor is rinch's model-first
 *    `editor-view`, not a `contenteditable`, so `document.getSelection()` describes
 *    nothing in it. The position comes from rinch's own `query_caret_position`, driven
 *    from the *model's* byte offset — see `editor_utils::caret_anchor`. If that route
 *    ever regresses the menu sits at the viewport origin, which is what the position
 *    assertion below catches.
 * 2. **`$` offers entities only, `@` reaches chapters too.**
 * 3. **The rail reads the edges in both directions**, including a chapter target.
 *
 * Typing goes through real keystrokes: the editor has no native editable, so
 * `.fill()` does nothing (same reason `typeInEditor` exists in helpers.ts).
 */

const NOTE_EDITOR = "#note-editor-main [data-pm-editor]";

/** Focus the note body and send real keystrokes. */
async function typeInNote(page: Page, text: string) {
  const surface = page.locator(NOTE_EDITOR);
  await surface.waitFor({ state: "visible", timeout: 15_000 });
  await surface.click();
  await page.keyboard.type(text);
}

/**
 * Leave the note editor the way the UI intends: the back arrow in its topbar, which
 * saves the body before navigating (`go_back_to_notes`). The sidebar's "Notes" entry
 * is *not* equivalent — it does not flush the note editor — so a spec that relies on
 * the body having been written must come back this way.
 */
async function backToNotes(page: Page) {
  await page.locator(".note-editor-topbar-left .rinch-action-icon").first().click();
  await expect(page.locator(".notes-pane-header")).toBeVisible();
}

/** Open a note from the notes tree by title and wait for its editor. */
async function openNote(page: Page, title: string) {
  if (await page.locator(".note-editor-pane .note-editor-topbar").isVisible()) {
    await backToNotes(page);
  } else {
    await openNotesPane(page);
  }
  await page.locator(".notes-tree .note-card-title", { hasText: title }).click();
  await expect(page.locator(".note-editor-pane")).toBeVisible();
  await page.locator(NOTE_EDITOR).waitFor({ state: "visible", timeout: 15_000 });
}

/** The menu rows currently offered, as `label` strings. */
function menuLabels(page: Page) {
  return page.locator("#sigil-menu .sigil-row .sigil-row-label");
}

test("sigils complete over real data and the rail shows both directions", async ({
  page,
}) => {
  await registerNewUser(page);
  await createBook(page, "Sigil Book");

  // A chapter to mention, an entity to reference, and a plain lore note that `$`
  // must not offer.
  await addChapter(page, "Three Doors");
  await createNote(page, "Vess");
  await createNote(page, "Vestibule lore");
  await createNote(page, "The siege");

  // Mark Vess as an entity through the facet strip. The strip patches only
  // `is_entity`; every other facet is left absent, meaning "leave alone".
  await openNote(page, "Vess");
  const entityFacet = page.locator("#note-facet-entity");
  await expect(entityFacet).toBeVisible();
  await expect(entityFacet).not.toHaveClass(/is-on/);
  await entityFacet.click();
  await expect(entityFacet).toHaveClass(/is-on/);

  // Seed a tag from a *different* note. `#` offers tags the book's saved notes
  // already carry, which is a note's link index — the note being edited has not been
  // saved yet, so its own half-typed tag is deliberately not offered back to it.
  await typeInNote(page, "#winter cast");

  // ── `$` offers entities only ───────────────────────────────────────────────
  await openNote(page, "The siege");
  await typeInNote(page, "Held by $Ve");

  const menu = page.locator("#sigil-menu");
  await expect(menu).toBeVisible();
  // Vess (entity) and the create-new row. "Vestibule lore" is lore, so `$` — which
  // is participation — must not offer it.
  await expect(menuLabels(page)).toHaveText(["Vess", "Ve"]);

  // The menu hangs off the caret, not the viewport origin. The caret is partway
  // into a line of prose inside the editor, so both coordinates must be well clear
  // of zero and inside the editor's box.
  const editorBox = await page.locator(NOTE_EDITOR).boundingBox();
  const menuBox = await menu.boundingBox();
  expect(editorBox).not.toBeNull();
  expect(menuBox).not.toBeNull();
  expect(menuBox!.x).toBeGreaterThan(editorBox!.x);
  expect(menuBox!.y).toBeGreaterThan(editorBox!.y);

  // Choosing inserts the real token.
  await page.locator("#sigil-menu .sigil-row", { hasText: "Vess" }).first().click();
  await expect(menu).toBeHidden();
  await expect(page.locator(NOTE_EDITOR)).toContainText("$Vess");

  // ── `@` reaches chapters as well as notes ──────────────────────────────────
  await typeInNote(page, "in @Three");
  await expect(menu).toBeVisible();
  await expect(menuLabels(page).first()).toHaveText("Three Doors");
  await expect(
    page.locator("#sigil-menu .sigil-row").first().locator(".sigil-row-kind"),
  ).toHaveText("chapter");
  await page.locator("#sigil-menu .sigil-row").first().click();
  // A token cannot hold a space, so the title's space is written as a hyphen and
  // folded back when the edge is derived.
  await expect(page.locator(NOTE_EDITOR)).toContainText("@Three-Doors");

  // ── `#` offers tags already used in the book ───────────────────────────────
  await typeInNote(page, " #win");
  await expect(menuLabels(page)).toHaveText(["winter", "win"]);
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();

  // ── The rail, both directions ──────────────────────────────────────────────
  // Leaving the note saves it, which is what derives the edges from the body.
  await openNote(page, "The siege");

  const outbound = page.locator(".note-rail-group.is-out");
  await expect(outbound.locator(".note-rail-label")).toHaveText([
    "Three Doors",
    "Vess",
  ]);


  // And the other way round: Vess knows the siege points at it.
  await openNote(page, "Vess");
  const inbound = page.locator(".note-rail-group.is-in");
  await expect(inbound.locator(".note-rail-label")).toHaveText(["The siege"]);
  await expect(page.locator(".note-rail-tag")).toHaveText(["#winter"]);

  // The backlink opens the note it names.
  await inbound.locator(".note-rail-row").first().click();
  await expect(page.locator(".note-editor-topbar-left")).toContainText("The siege");
});

test("the completion list can be chosen by touch on a phone", async ({ page }) => {
  // rinch fires `onclick` on **pointerdown**, which has broken scrollable tap-lists
  // before (rinch #104) — and a completion list is exactly that shape. A real tap is
  // the only way to prove it: a synthetic `click()` takes a different path entirely.
  await registerNewUser(page);
  await createBook(page, "Touch Book");
  await createNote(page, "Vess");
  await createNote(page, "The siege");

  await openNote(page, "Vess");
  await page.locator("#note-facet-entity").click();
  await expect(page.locator("#note-facet-entity")).toHaveClass(/is-on/);

  // Narrow only once the note is open: the sidebar the notes tree is reached through
  // collapses at this width, and getting there is not what is under test.
  await openNote(page, "The siege");
  await page.setViewportSize({ width: 390, height: 844 });

  // The rail stacks under the prose at this width rather than squeezing it out.
  const railBox = await page.locator(".note-rail").boundingBox();
  const bodyBox = await page.locator(".note-editor-body").boundingBox();
  expect(railBox!.y).toBeGreaterThan(bodyBox!.y);

  await typeInNote(page, "Held by $Ve");
  await expect(page.locator("#sigil-menu")).toBeVisible();
  await expect(menuLabels(page).first()).toHaveText("Vess");

  // Tap the first row with genuine touch events.
  const session = await page.context().newCDPSession(page);
  const row = await page.locator("#sigil-menu .sigil-row").first().boundingBox();
  const x = row!.x + row!.width / 2;
  const y = row!.y + row!.height / 2;
  await session.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x, y }],
  });
  await session.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });

  await expect(page.locator("#sigil-menu")).toBeHidden();
  await expect(page.locator(NOTE_EDITOR)).toContainText("$Vess");
});

test("a chapter shows which notes say it dramatises them", async ({ page }) => {
  // The chapter side of an `@` edge. A chapter body is prose and is never scanned for
  // sigils, so this is the notes' index read backwards — and the strip stays unmounted
  // when nothing points here, rather than holding a row open to say so.
  await registerNewUser(page);
  const bookId = await createBook(page, "Both Ways");
  await addChapter(page, "Three Doors");
  await createNote(page, "The siege");

  await openChapter(page, "Three Doors");
  await expect(page.locator(".chapter-backlinks")).toHaveCount(0);

  await openNote(page, "The siege");
  await typeInNote(page, "dramatised in @Three-Doors");
  await page.locator("#sigil-menu .sigil-row").first().click();
  await backToNotes(page);

  // Reload so the strip is fed from the server's derived index, not just what this
  // tab happened to have in memory.
  await page.goto(`/book/${bookId}`);
  await openChapter(page, "Three Doors");
  const strip = page.locator(".chapter-backlinks");
  await expect(strip.locator(".chapter-backlink")).toHaveText(["The siege"]);

  // And it opens the note that named it.
  await strip.locator(".chapter-backlink").first().click();
  await expect(page.locator(".note-editor-topbar-left")).toContainText("The siege");
});

test("notes arriving mid-session do not disturb the chapter editor", async ({ page }) => {
  // The "Notes here" strip reads `store.notes` inside the chapter editor's render, so
  // it is fair to ask whether a notes list landing mid-sentence rebuilds the editor
  // under the caret — which would reset the on-screen keyboard's mirror and lose the
  // author's place. It does not: `rsx!`'s `if` desugars to rinch's `show_dom`, which
  // swaps only its own subtree and only when the boolean actually flips.
  //
  // This pins that down where it would otherwise be an argument about generated code.
  // The notes response is held until the caret is live and the strip's condition is
  // about to flip false -> true, so the flip is deterministic rather than a race.
  await registerNewUser(page);
  const bookId = await createBook(page, "Mid-session");
  await addChapter(page, "Chapter One");

  const note = await (
    await page.request.post(`/api/books/${bookId}/notes`, {
      data: { title: "Vess", parent_id: null, color: null },
    })
  ).json();
  await page.request.put(`/api/books/${bookId}/notes/${note.id}`, {
    data: {
      content: JSON.stringify({
        type: "doc",
        content: [{ type: "paragraph", content: [{ type: "text", text: "@Chapter-One" }] }],
      }),
    },
  });

  let release: () => void = () => {};
  const held = new Promise<void>((r) => (release = r));
  await page.route(`**/api/books/${bookId}/notes`, async (route) => {
    if (route.request().method() !== "GET") return route.continue();
    await held;
    await route.continue();
  });

  await page.goto(`/book/${bookId}`);
  await openChapter(page, "Chapter One");
  await typeInEditor(page, "hello word here");

  // Stamp the live editor surface so a rebuild is detectable at all.
  await page.evaluate(() => {
    (document.querySelector("#editor-main [data-pm-editor]") as any).__probe = "alive";
  });
  const caret = () =>
    page.evaluate(
      () => (document.querySelector("[data-pm-capture]") as HTMLTextAreaElement)?.selectionStart,
    );
  expect(await caret()).toBe(15);
  await expect(page.locator(".chapter-backlinks")).toHaveCount(0);

  // Let the notes land: the strip appears, and nothing else may move.
  release();
  await expect(page.locator(".chapter-backlinks")).toHaveCount(1);

  expect(
    await page.evaluate(
      () => (document.querySelector("#editor-main [data-pm-editor]") as any).__probe ?? "REBUILT",
    ),
  ).toBe("alive");
  expect(await caret()).toBe(15);

  // And the next keystroke still lands at the caret, not at the start of the block.
  await page.keyboard.type("!");
  await expect(page.locator("#editor-main [data-pm-editor]")).toContainText("hello word here!");
});
