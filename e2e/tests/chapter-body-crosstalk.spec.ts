import { test, expect } from "@playwright/test";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * Switching chapters must never let one chapter's text reach another.
 *
 * Guards two separate defects, both reported live as chapters appearing swapped,
 * blank, or overwritten with another chapter's prose.
 *
 * **1. A load recorded into the previous chapter's CRDT (deterministic).** An
 * attached collaboration session records document changes into that document's
 * CRDT — and a load *is* a document change; `start_collaboration_guest` relies on
 * this, loading before attaching so the load isn't recorded back. Chapter
 * switching loaded the next chapter's content into an editor still bound to the
 * previous chapter's session, writing B's content into **A's** local doc. Since a
 * local doc is adopted in preference to the server copy on reopen, chapters came
 * back swapped or blank while the server still held the right text. Fixed by
 * detaching inside `editor_utils`' load functions, the choke point every load
 * passes through.
 *
 * **2. A stale attach resuming against the wrong chapter (race).** Attaching is
 * asynchronous — open the backend, read the manifest, list deltas — and every
 * path writes to the editor. Nothing re-checked which chapter was open once those
 * awaits resumed, so switching mid-attach let the earlier chapter's continuation
 * load its content into the editor now showing the later one; the page's own
 * guard (`loaded_chapter_id`) was satisfied, so autosave persisted it over the
 * open chapter. Fixed by `local_store`'s surface binding.
 *
 * Both are browser-only: natively the storage futures resolve on first poll, so
 * an attach runs start-to-finish synchronously. CPU throttling below widens the
 * window a fast local IndexedDB would otherwise close.
 */

const ALPHA = "Alpha chapter prose about a lighthouse.";
const BETA = "Beta chapter prose about a harbour.";

test("editor: rapid chapter switching never mixes chapter bodies", async ({ page }) => {
  await registerNewUser(page);
  await createBook(page, "Crosstalk Novel");
  await addChapter(page, "Alpha");
  await addChapter(page, "Beta");

  await openChapter(page, "Alpha");
  await typeInEditor(page, ALPHA);
  await openChapter(page, "Beta");
  await typeInEditor(page, BETA);

  // Leave the editor so the debounced autosave flushes, then reload so both
  // bodies come back from the server and each has a local doc on disk (the
  // adopt path, which is the one that replaces the editor document).
  await openChapter(page, "Alpha");
  await expect(page.locator("#editor-main")).toContainText(ALPHA);
  await page.reload();

  // Slow the main thread so the attach's IndexedDB round-trips stay in flight
  // across the next click — this is what makes the race observable at all.
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 20 });

  // Switch back and forth without waiting for anything to settle. Each switch
  // starts an attach that the next one supersedes.
  for (let i = 0; i < 6; i++) {
    await page.locator(".sidebar-chapter-item", { hasText: "Alpha" }).click();
    await page.locator(".sidebar-chapter-item", { hasText: "Beta" }).click();
  }

  // Whatever the editor settles on, it must be Beta's body — never Alpha's.
  await openChapter(page, "Beta");
  await expect(page.locator("#editor-main")).toContainText(BETA);
  await expect(page.locator("#editor-main")).not.toContainText(ALPHA);

  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 1 });

  // And nothing was persisted over either chapter: a fresh load (server state,
  // then the local doc) shows each chapter's own prose.
  await page.reload();
  await openChapter(page, "Alpha");
  await expect(page.locator("#editor-main")).toContainText(ALPHA);
  await expect(page.locator("#editor-main")).not.toContainText(BETA);

  await openChapter(page, "Beta");
  await expect(page.locator("#editor-main")).toContainText(BETA);
  await expect(page.locator("#editor-main")).not.toContainText(ALPHA);
});
