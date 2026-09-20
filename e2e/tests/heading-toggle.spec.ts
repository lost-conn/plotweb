import { test, expect, Locator, Page } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, returnChrome, typeInEditor } from "./helpers";

/**
 * A heading button on a block that already is that heading turns it back
 * into a paragraph.
 *
 * rinch's `setHeadingN` is a set, not a toggle: on a block already at that
 * level it is a no-op, so the toolbar's highlighted H1/H2/H3 button used to do
 * nothing when clicked, and the only way out of a heading was the paragraph
 * shortcut. The toolbar now runs `setParagraph` when the current block is
 * already that level (`heading_click!` in `editor_utils.rs`).
 *
 * The buttons carry no text or tooltip hook, so they are targeted by their
 * Tabler icon's first path — the digit stroke is unique per level — the same
 * way `scene-break.spec.ts` finds its button.
 */

const H1_PATH = "M19 18v-8l-2 2";
const H2_PATH = "M17 12a2 2 0 1 1 4 0c0 .591 -.417 1.318 -.816 1.858l-3.184 4.143l4 0";

const headingButton = (page: Page, d: string): Locator =>
  page.locator(`.editor-layout .toolbar .rinch-action-icon:has(path[d="${d}"])`);

/** `fmt_button` flips the ActionIcon variant from `subtle` to `light` when active. */
async function isActive(btn: Locator): Promise<boolean> {
  const cls = (await btn.getAttribute("class")) ?? "";
  return cls.includes("rinch-action-icon--light");
}

/** Tag names of the real block nodes in the editor, in document order. */
const blockTags = (page: Page) =>
  page.locator("#editor-main [data-pm-editor]").evaluate((root) =>
    Array.from(root.children)
      .filter((el) => el.hasAttribute("data-pm-type"))
      .map((el) => el.tagName.toLowerCase()),
  );

test("the active heading button turns the block back into a paragraph", async ({ page }) => {
  await registerNewUser(page);
  await createBook(page, "Heading Novel");
  await addChapter(page, "Chapter One");
  await openChapter(page, "Chapter One");

  await typeInEditor(page, "A line that will become a heading.");
  // Every toolbar click is an edit, which collapses the chrome again, so each
  // click below is preceded by `returnChrome` (see text-alignment.spec.ts).

  const h2 = headingButton(page, H2_PATH);
  const h1 = headingButton(page, H1_PATH);
  await expect(h2).toBeVisible();

  // Paragraph -> H2.
  await returnChrome(page);
  await h2.click();
  await expect.poll(() => blockTags(page)).toEqual(["h2"]);
  await expect.poll(() => isActive(h2)).toBe(true);

  // H2 again -> paragraph (the bug: this used to be a no-op).
  await returnChrome(page);
  await h2.click();
  await expect.poll(() => blockTags(page)).toEqual(["p"]);
  await expect.poll(() => isActive(h2)).toBe(false);

  // Switching levels is still a set: H1 then H2 lands on H2, not a paragraph.
  await returnChrome(page);
  await h1.click();
  await expect.poll(() => blockTags(page)).toEqual(["h1"]);
  await returnChrome(page);
  await h2.click();
  await expect.poll(() => blockTags(page)).toEqual(["h2"]);
  await expect.poll(() => isActive(h1)).toBe(false);
  await expect.poll(() => isActive(h2)).toBe(true);

  // The text survived every change.
  await expect(page.locator("#editor-main [data-pm-editor]")).toContainText(
    "A line that will become a heading.",
  );
});
