import { test, expect, Page } from "@playwright/test";
import { createBook, createNote, openNotesPane, registerNewUser } from "./helpers";

/**
 * A book's calendar, and a note's time typed through it (notes revamp, card 4).
 *
 * What is guarded here:
 *
 * 1. **A contemporary book needs no setup.** The default, Gregorian-shaped calendar
 *    reads "1817 Mar 4" without the calendar screen ever being opened.
 * 2. **An invented calendar works with no special case.** "The Accord" — four named
 *    seasons, 320 days, sixteen bells — is typed into the calendar screen, and notes
 *    are then dated in it.
 * 3. **All five time states round-trip** — exact, approximate, open-ended, relative,
 *    undated — through the server and a reload, and read the same in the facet strip
 *    and the tree gutter.
 * 4. **The calendar link in the time field is an exit** from the note editor, so it
 *    must flush the pending body edit first (`pages/book/flush.rs`).
 * 5. **The `project_notes` hazard.** A span the local `book:` document has not heard
 *    of must still show; a span this device wrote must survive a note-list refetch
 *    even when the server never received it.
 * 6. **Touch.** The strip's time control and the field's buttons, tapped.
 */

const NOTE_EDITOR = "#note-editor-main [data-pm-editor]";
const YEAR = 31_536_000;

function row(page: Page, title: string) {
  return page.locator(".note-row", {
    has: page.locator(".note-card-title", { hasText: title }),
  });
}

async function openNote(page: Page, title: string) {
  await page.locator(".notes-tree .note-card-title", { hasText: title }).click();
  await page.locator(NOTE_EDITOR).waitFor({ state: "visible", timeout: 15_000 });
  await expect(page.locator(".note-editor-topbar")).toContainText(title);
}

async function backToNotes(page: Page) {
  await page.locator(".note-editor-topbar .rinch-action-icon").first().click();
  await expect(page.locator(".notes-pane-header")).toBeVisible();
}

/** Type into a plain input at a human pace and check it arrived — see `typeInNote`. */
async function typeInto(page: Page, selector: string, text: string) {
  const input = page.locator(selector);
  await input.click();
  await input.fill("");
  await input.pressSequentially(text, { delay: 10 });
  await expect(input).toHaveValue(text);
}

/** Date the open note by typing into its time field, and wait for the strip to say so. */
async function setTime(page: Page, text: string, reads: string) {
  await page.locator("#note-facet-when").click();
  await expect(page.locator("#note-time-editor")).toBeVisible();
  await typeInto(page, "#note-time-editor input", text);
  await page.locator("#note-time-editor input").press("Enter");
  await expect(page.locator("#note-time-editor")).toBeHidden();
  await expect(page.locator("#note-facet-when")).toHaveText(reads);
}

/** Set one field of one row of the calendar form. */
async function calField(page: Page, rowIndex: number, field: string, value: string) {
  await typeInto(page, `.cal-row >> nth=${rowIndex} >> input[data-field="${field}"]`, value);
}

/** Turn the default calendar into the Accord through the calendar screen. */
async function defineTheAccord(page: Page) {
  await openNotesPane(page);
  await page.locator("#notes-calendar button").click();
  await expect(page.locator("#calendar-pane")).toBeVisible();
  // Four rows to start with: the default Year / Month / Day / Hour.
  await expect(page.locator(".cal-row")).toHaveCount(4);
  await expect(page.locator(".calendar-preview")).toHaveText("Dates read like: 1206 May 14 1h");

  await typeInto(page, "#calendar-name", "The Accord");
  await calField(page, 0, "written_as", "yr {n}");
  await calField(page, 1, "name", "Season");
  await calField(page, 1, "defined_as", "1/4 Year");
  await calField(page, 1, "written_as", ", {name}");
  await calField(page, 1, "names", "wet, dry, high, low");
  await calField(page, 1, "shown_when", "< 40 Years");
  await calField(page, 2, "defined_as", "1/320 Year");
  await calField(page, 2, "written_as", ", day {n}");
  await calField(page, 2, "shown_when", "< 2 Years");
  await calField(page, 3, "name", "Bell");
  await calField(page, 3, "defined_as", "1/16 Day");
  await calField(page, 3, "written_as", ", bell {n}");
  await calField(page, 3, "counts_from", "1");
  await calField(page, 3, "shown_when", "< 20 Days");

  await expect(page.locator(".calendar-preview")).toHaveText(
    "Dates read like: yr 1206, dry, day 39, bell 7",
  );
  await page.locator("#calendar-save button").click();
  await expect(page.locator(".calendar-status")).toHaveText("Saved");
}

async function bookJson(page: Page, bookId: string) {
  const r = await page.request.get(`/api/books/${bookId}`);
  expect(r.ok()).toBeTruthy();
  return r.json();
}

async function notesJson(page: Page, bookId: string): Promise<Array<Record<string, any>>> {
  const r = await page.request.get(`/api/books/${bookId}/notes`);
  expect(r.ok()).toBeTruthy();
  return (await r.json()).notes;
}

test("a contemporary book dates a note with no setup at all", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Default Calendar Novel");
  await createNote(page, "The wedding");
  await openNote(page, "The wedding");

  await expect(page.locator("#note-facet-when")).toHaveText("Undated");
  // The hint names the default calendar's parts, so the author can see the shape.
  await page.locator("#note-facet-when").click();
  await expect(page.locator(".note-time-help")).toContainText("Year · Month · Day · Hour");
  await page.locator("#note-time-cancel button").click();

  await setTime(page, "1817 March 4", "1817 Mar 4");
  // The preview named the depth in the book's own words before it was committed; the
  // rail now says when too.
  await expect(page.locator(".note-rail-when")).toHaveText("1817 Mar 4");

  // The book never got a calendar of its own: it is still on the default.
  expect((await bookJson(page, bookId)).calendar).toBeUndefined();

  await page.reload();
  await openNotesPane(page);
  await expect(row(page, "The wedding").locator(".note-when")).toHaveText("1817 Mar 4");
});

test("an invented calendar: all five time states round-trip through the server and a reload", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await registerNewUser(page);
  const bookId = await createBook(page, "The Accord Cycle");
  for (const title of ["The breach", "The first frost", "The exile", "The parley", "Amberwork"]) {
    await createNote(page, title);
  }

  await defineTheAccord(page);
  await page.locator("#calendar-pane .calendar-head .rinch-action-icon").click();
  await expect(page.locator(".notes-pane-header")).toBeVisible();

  // Exact: an instant to the bell, running to the next day.
  await openNote(page, "The breach");
  await setTime(
    page,
    "yr 1206, dry, day 12, bell 9 – yr 1206, dry, day 13",
    "yr 1206, dry, day 12, bell 9 – yr 1206, dry, day 13",
  );
  await backToNotes(page);

  // Approximate, typed loosely.
  await openNote(page, "The first frost");
  await page.locator("#note-facet-when").click();
  await typeInto(page, "#note-time-editor input", "c. 1206 low");
  await expect(page.locator(".note-time-preview")).toHaveText("Approximate · known to the Season");
  await page.locator("#note-time-set button").click();
  await expect(page.locator("#note-facet-when")).toHaveText("~yr 1206, low");
  await backToNotes(page);

  // Open-ended.
  await openNote(page, "The exile");
  await setTime(page, "1198 onward", "yr 1198 –");
  await backToNotes(page);

  // Relative, by the title of another note.
  await openNote(page, "The parley");
  await setTime(page, "after the breach", "after The breach");
  await backToNotes(page);

  // Undated: dated, then taken back with the Undated button.
  await openNote(page, "Amberwork");
  await setTime(page, "1300", "yr 1300");
  await page.locator("#note-facet-when").click();
  await page.locator("#note-time-clear button").click();
  await expect(page.locator("#note-facet-when")).toHaveText("Undated");

  // A mistake is refused with a reason, and nothing is written.
  await page.locator("#note-facet-when").click();
  await typeInto(page, "#note-time-editor input", "yr 1206, spring");
  await page.locator("#note-time-editor input").press("Enter");
  await expect(page.locator(".note-time-preview.is-error")).toContainText("spring");
  await page.locator("#note-time-cancel button").click();
  await expect(page.locator("#note-facet-when")).toHaveText("Undated");
  await backToNotes(page);

  const gutters = {
    "The breach": "yr 1206, dry, day 12, bell 9 – yr 1206, dry, day 13",
    "The first frost": "~yr 1206, low",
    "The exile": "yr 1198 –",
    "The parley": "after The breach",
    Amberwork: "",
  };
  const checkGutters = async () => {
    for (const [title, label] of Object.entries(gutters)) {
      await expect(row(page, title).locator(".note-when")).toHaveText(label);
    }
  };
  await checkGutters();

  // The server holds exactly what was typed, in ticks — through the book's calendar.
  const book = await bookJson(page, bookId);
  expect(book.calendar.name).toBe("The Accord");
  expect(book.calendar.units.map((u: { name: string }) => u.name)).toEqual([
    "Year",
    "Season",
    "Day",
    "Bell",
  ]);
  const byTitle = new Map((await notesJson(page, bookId)).map((n) => [n.title, n]));
  const frost = byTitle.get("The first frost")!;
  expect(frost.span.approximate).toBe(true);
  expect(frost.span.start).toEqual({ tick: 1206 * YEAR + (3 * YEAR) / 4, precision: 1 });
  expect(byTitle.get("The exile")!.span.open_ended).toBe(true);
  expect(byTitle.get("The breach")!.span.start.precision).toBe(3);
  expect(byTitle.get("The breach")!.span.end.precision).toBe(2);
  expect(byTitle.get("The parley")!.relative).toEqual({
    relation: "after",
    note_id: byTitle.get("The breach")!.id,
  });
  expect(byTitle.get("The parley")!.span).toBeUndefined();
  expect(byTitle.get("Amberwork")!.span).toBeUndefined();
  expect(byTitle.get("Amberwork")!.relative).toBeUndefined();

  // And a reload — the local document, the server and the calendar together.
  await page.reload();
  await openNotesPane(page);
  await checkGutters();
  await openNote(page, "The first frost");
  await expect(page.locator("#note-facet-when")).toHaveText("~yr 1206, low");
  // The field is prefilled with what it reads as, so an unchanged Enter re-dates
  // nothing.
  await page.locator("#note-facet-when").click();
  await expect(page.locator("#note-time-editor input")).toHaveValue("~yr 1206, low");
  await page.locator("#note-time-editor input").press("Enter");
  await expect(page.locator("#note-facet-when")).toHaveText("~yr 1206, low");

  // The calendar screen reopens on the Accord, and resetting it re-reads every date
  // through the default calendar without touching a single stored tick.
  await page.locator("#note-facet-when").click();
  await page.locator("#note-time-calendar").click();
  await expect(page.locator("#calendar-name")).toHaveValue("The Accord");
  await page.locator("#calendar-reset button").click();
  await expect(page.locator(".calendar-status")).toHaveText("Saved");
  await page.locator("#calendar-pane .calendar-head .rinch-action-icon").click();
  // Back to the note it was opened from.
  await expect(page.locator(".note-editor-topbar")).toContainText("The first frost");
  await expect(page.locator("#note-facet-when")).toHaveText("~1206 Oct");
  expect((await bookJson(page, bookId)).calendar).toBeUndefined();
});

test("the time field's calendar link is an exit that saves the note body", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Calendar Exit");
  await createNote(page, "The siege");
  await openNote(page, "The siege");

  const writes: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "PUT" && /\/api\/books\/[^/]+\/notes\/[^/]+$/.test(r.url())) {
      writes.push(r.url());
    }
  });
  const text = "Written just before reaching for the calendar.";
  const surface = page.locator(NOTE_EDITOR);
  await surface.click();
  await page.keyboard.type(text, { delay: 10 });
  await expect(surface).toContainText(text);
  expect(writes, "the 800ms debounce must still be pending").toEqual([]);

  await page.locator("#note-facet-when").click();
  await page.locator("#note-time-calendar").click();
  await expect(page.locator("#calendar-pane")).toBeVisible();
  expect(writes, "leaving for the calendar must write the note").not.toEqual([]);

  await page.goto(`/book/${bookId}`);
  await openNotesPane(page);
  await openNote(page, "The siege");
  await expect(page.locator(NOTE_EDITOR)).toContainText(text);
});

test("a span the local document has not heard of still shows, and one it holds survives a refetch", async ({
  page,
}) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Out Of Band");
  await createNote(page, "The siege");
  await createNote(page, "The parley");
  // The book has been opened, so its local `book:` document exists and was seeded
  // with both notes undated.
  const ids = new Map((await notesJson(page, bookId)).map((n) => [n.title, n.id as string]));

  // 1. Written over HTTP after the first open — another device, or a script.
  const put = await page.request.put(`/api/books/${bookId}/notes/${ids.get("The siege")}`, {
    data: {
      span: {
        start: { tick: 1206 * YEAR, precision: 0 },
        approximate: false,
        open_ended: false,
      },
    },
  });
  expect(put.ok()).toBeTruthy();
  await page.reload();
  await openNotesPane(page);
  await expect(row(page, "The siege").locator(".note-when")).toHaveText("1206");

  // 2. Dated on this device while the server refuses the write. The local document
  // holds it; a tree edit then refetches the note list, which has not heard of it.
  await page.route(`**/api/books/${bookId}/notes/${ids.get("The parley")}`, (route) =>
    route.request().method() === "PUT" ? route.abort() : route.continue(),
  );
  await openNote(page, "The parley");
  await setTime(page, "1207", "1207");
  await backToNotes(page);
  await createNote(page, "The long winter");
  await expect(row(page, "The parley").locator(".note-when")).toHaveText("1207");
  await page.unroute(`**/api/books/${bookId}/notes/${ids.get("The parley")}`);
  // Still there after a reload: the refetch did not delete it from the document.
  await page.reload();
  await openNotesPane(page);
  await expect(row(page, "The parley").locator(".note-when")).toHaveText("1207");
  await expect(row(page, "The siege").locator(".note-when")).toHaveText("1206");
});

test.describe("on a phone", () => {
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

  test("the time control and its buttons work by tap", async ({ page }) => {
    await registerNewUser(page);
    await createBook(page, "Touch Calendar");
    await createNote(page, "The ferry");
    await page.locator(".notes-tree .note-card-title", { hasText: "The ferry" }).tap();
    await page.locator(NOTE_EDITOR).waitFor({ state: "visible", timeout: 15_000 });

    await page.locator("#note-facet-when").tap();
    await expect(page.locator("#note-time-editor")).toBeVisible();
    await typeInto(page, "#note-time-editor input", "~1990 Jun");
    await page.locator("#note-time-set button").tap();
    await expect(page.locator("#note-facet-when")).toHaveText("~1990 Jun");

    await page.locator("#note-facet-when").tap();
    await page.locator("#note-time-cancel button").tap();
    await expect(page.locator("#note-time-editor")).toBeHidden();
    await expect(page.locator("#note-facet-when")).toHaveText("~1990 Jun");
  });
});
