import { test, expect, Page } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { addChapter, createBook, openChapter, registerNewUser, typeInEditor } from "./helpers";

/**
 * What an author sees when a save reaches the server and is not stored.
 *
 * Under cutover a syncing client tells the server "my sync engine is carrying this
 * body", and the server withholds the write from git. If the canonical document is one
 * the server cannot read, honouring that leaves both writers standing down and the edit
 * lives only in the browser. In production the response was `200` throughout and the
 * editor said "Saved" for two days.
 *
 * The server now takes the write to git and reports the degradation; this is the half
 * that decides whether anyone finds out.
 *
 * One writer briefly made this unreachable rather than fixed: the client stopped sending
 * body content for a cut-over book at all, so the server had nothing to take to git and
 * the receipt carried no warning — the save landed nowhere, quietly, which is the very
 * thing this file exists to catch. The client now answers a non-durable receipt once,
 * with the content (`save_chapter_body`), which is what puts the alert below back.
 */

// Needs a server started with `PLOTWEB_CUTOVER_BOOKS=*` — the flag is what makes a
// `sync_owned` declaration mean anything at all.
test.skip(
  !process.env.PLOTWEB_E2E_CUTOVER,
  "requires a cut-over server — run `npm run test:cutover`",
);

/** The throwaway state directory the launch script published. */
function stateDir(): string {
  const file = path.join(__dirname, "..", ".e2e-state");
  return fs.readFileSync(file, "utf8").trim();
}

/**
 * Make every canonical body unreadable, and keep it that way.
 *
 * The corruption is the production shape (a document in a projection this build cannot
 * open). Dropping write permission on the store afterwards is what makes the test
 * deterministic rather than a race: a syncing client would otherwise notice the
 * unreadable document, adopt it, and repair the very condition under test.
 */
function breakCanonicalStore(): string {
  const crdt = path.join(stateDir(), "crdt");
  for (const name of fs.readdirSync(crdt)) {
    if (name.includes("chapter%3A") && name.endsWith("snapshot")) {
      fs.writeFileSync(path.join(crdt, name), "a blob from before a CRDT change");
    }
  }
  fs.chmodSync(crdt, 0o555);
  return crdt;
}

async function editorSaysSaved(page: Page) {
  await expect(page.locator(".save-indicator")).toHaveText("Saved", { timeout: 15_000 });
}

test("a save the server cannot store is shown to the author, not reported as saved", async ({
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);

  // Sync on: that is what makes the client declare `sync_owned`, which is the whole
  // premise of the bug.
  const context = await browser.newContext();
  await context.addInitScript(() => {
    window.localStorage.setItem("plotweb_sync", "1");
  });
  const page = await context.newPage();
  await page.goto(baseURL!);

  await registerNewUser(page);
  await createBook(page, "Degraded Save");
  await addChapter(page, "One");
  await openChapter(page, "One");
  await typeInEditor(page, "the first paragraph");
  await editorSaysSaved(page);

  const crdt = breakCanonicalStore();
  try {
    await typeInEditor(page, " and more written after the break");

    // The point of the change: the author is told, in the editor, that this writing is
    // not where they think it is. Scoped to the editor pane — the note editor carries
    // the same alert, so an unscoped match is ambiguous rather than wrong.
    const editorPane = page.locator(".editor-layout");
    await expect(editorPane.getByText("Your writing isn't reaching the server")).toBeVisible({
      timeout: 20_000,
    });
    await expect(editorPane.getByText(/syncing is paused for it/)).toBeVisible();
    await expect(editorPane.getByRole("button", { name: "Retry save" })).toBeVisible();

    // The indicator still reads "Saved", and that is correct rather than a leftover:
    // git did take this write. "Saved" answers where the words are, the alert answers
    // what stopped working. Asserted so a change that quietly downgrades a stored save
    // to an error has to argue with this line.
    await expect(page.locator(".save-indicator")).toHaveText("Saved");

    // And the writing itself survived: degraded is not the same as lost, so a reload
    // has to bring the words back.
    await page.reload();
    await openChapter(page, "One");
    await expect(page.locator("#editor-main [data-pm-editor]")).toContainText(
      "written after the break",
    );
  } finally {
    fs.chmodSync(crdt, 0o755);
    await context.close();
  }
});
