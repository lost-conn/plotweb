import { test, expect, Locator, Page } from "@playwright/test";
import { createBook, createNote, openNotesPane, registerNewUser } from "./helpers";

/**
 * Notes drag-and-drop coverage.
 *
 * PlotWeb's notes tree uses rinch-web's *synthetic* pointer DnD (delegated at the
 * document level via pointerdown/move/up — NOT native HTML5 drag). A mouse drag
 * activates once the pointer moves past 5 CSS px; touch/pen activate via a 350ms
 * long-press hold. Dropping over a note card splits it into vertical thirds:
 * top => insert before, bottom => insert after, middle => nest as a child. The
 * drop issues a PUT to /api/books/{id}/notes/move and then refetches the tree, so
 * the DOM only reorders after a network round-trip — every assertion below leans
 * on Playwright's auto-waiting rather than fixed sleeps.
 *
 * This guards a regression in notes DnD under rinch-web.
 */

/** Locate the note card (the draggable `.note-card`) that owns a given title. */
function noteCard(page: Page, title: string): Locator {
  return page.locator(".note-card", {
    has: page.locator(".note-card-title", { hasText: title }),
  });
}

/**
 * Perform a synthetic *mouse* drag of `source` to a point produced by
 * `targetPoint`.
 *
 * The target point is resolved *after* the drag activates, because activating a
 * drag makes every `.note-drop-zone` visible (8px each), which shifts the target
 * cards downward — a point captured before pressing would be stale.
 */
async function mouseDragNote(
  page: Page,
  source: Locator,
  targetPoint: () => Promise<{ x: number; y: number }>,
) {
  const s = await source.boundingBox();
  if (!s) throw new Error("source card has no bounding box");
  const cx = s.x + s.width / 2;
  const cy = s.y + s.height / 2;

  await page.mouse.move(cx, cy);
  await page.mouse.down();
  // Cross the 5px WEB_DRAG_THRESHOLD to activate the drag (fires ondragstart).
  await page.mouse.move(cx + 6, cy, { steps: 2 });
  // Now that drop zones have expanded, resolve the (shifted) target point.
  const p = await targetPoint();
  await page.mouse.move(p.x, p.y, { steps: 8 });
  await page.mouse.up();
}

/**
 * Dispatch a single synthetic *touch* PointerEvent on the note card that owns
 * `title`, at a vertical fraction `frac` of the card (0 = top, 1 = bottom). The
 * card's rect is read live so it reflects the drop-zone expansion mid-drag.
 *
 * rinch's touch DnD arms a real 350ms long-press setTimeout on pointerdown; the
 * caller waits it out between the down and the move (see the test below). Because
 * these are dispatched (not real) pointers, `setPointerCapture` fails and the
 * drag falls back to using the event target — so each event must be dispatched on
 * the card actually under the finger.
 */
async function touchEventOnNote(
  page: Page,
  type: "pointerdown" | "pointermove" | "pointerup",
  title: string,
  frac: number,
  pointerId = 1,
) {
  await page.evaluate(
    ({ type, title, frac, pointerId }) => {
      const card = Array.from(document.querySelectorAll(".note-card")).find(
        (c) => c.querySelector(".note-card-title")?.textContent?.trim() === title,
      );
      if (!card) throw new Error("note card not found: " + title);
      const r = card.getBoundingClientRect();
      const ev = new PointerEvent(type, {
        pointerId,
        pointerType: "touch",
        isPrimary: true,
        bubbles: true,
        cancelable: true,
        clientX: r.left + r.width / 2,
        clientY: r.top + r.height * frac,
        button: 0,
        // Contact stays down for down/move (buttons bit 0 set), lifts on up.
        buttons: type === "pointerup" ? 0 : 1,
      });
      card.dispatchEvent(ev);
    },
    { type, title, frac, pointerId },
  );
}

test("reorder root notes via mouse drag, and it persists", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Notes Reorder Novel");

  await createNote(page, "Alpha");
  await createNote(page, "Beta");

  // Initial DOM order: Alpha, then Beta.
  await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
    "Alpha",
    "Beta",
  ]);

  // Drag Alpha onto the bottom third of Beta => Alpha moves after Beta.
  await mouseDragNote(page, noteCard(page, "Alpha"), async () => {
    const b = await noteCard(page, "Beta").boundingBox();
    if (!b) throw new Error("Beta card has no bounding box");
    return { x: b.x + b.width / 2, y: b.y + b.height * 0.8 };
  });

  // The reorder lands only after the PUT + tree refetch — auto-wait for it.
  await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
    "Beta",
    "Alpha",
  ]);

  // Reload proves the move hit the server (not just optimistic UI).
  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
    "Beta",
    "Alpha",
  ]);
});

test("nest a note as a child via mouse drag, and it persists", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Notes Nesting Novel");

  await createNote(page, "Parent");
  await createNote(page, "Child");

  // Both start at the root (no nesting yet).
  await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
    "Parent",
    "Child",
  ]);
  await expect(page.locator(".note-children")).toHaveCount(0);

  // Drag Child onto the middle third of Parent => Child nests under Parent.
  await mouseDragNote(page, noteCard(page, "Child"), async () => {
    const b = await noteCard(page, "Parent").boundingBox();
    if (!b) throw new Error("Parent card has no bounding box");
    return { x: b.x + b.width / 2, y: b.y + b.height * 0.5 };
  });

  // Child now lives inside Parent's `.note-children` subtree.
  await expect(page.locator(".note-children .note-card-title")).toHaveText([
    "Child",
  ]);
  // Both notes still exist, just nested now.
  await expect(page.locator(".note-card-title")).toHaveCount(2);

  // Persisted across a reload.
  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await expect(page.locator(".note-children .note-card-title")).toHaveText([
    "Child",
  ]);
  await expect(page.locator(".note-card-title")).toHaveCount(2);
});

test.describe("touch", () => {
  test.use({ hasTouch: true });

  test("reorder root notes via a long-press touch drag", async ({ page }) => {
    await registerNewUser(page);
    const bookId = await createBook(page, "Notes Touch Novel");

    await createNote(page, "One");
    await createNote(page, "Two");

    await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
      "One",
      "Two",
    ]);

    // Long-press "One": pointerdown, then hold still past the 350ms long-press
    // window (no movement, or rinch treats it as a scroll and aborts).
    await touchEventOnNote(page, "pointerdown", "One", 0.5);
    await page.waitForTimeout(400);
    // The drag is now active; move onto the bottom third of "Two", then release.
    await touchEventOnNote(page, "pointermove", "Two", 0.85);
    await touchEventOnNote(page, "pointerup", "Two", 0.85);

    // One moved after Two (PUT + refetch) — auto-wait for the reordered DOM.
    await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
      "Two",
      "One",
    ]);

    // Persisted server-side.
    await page.goto(`/book/${bookId}`);
    await openNotesPane(page);
    await expect(page.locator(".notes-tree .note-card-title")).toHaveText([
      "Two",
      "One",
    ]);
  });
});
