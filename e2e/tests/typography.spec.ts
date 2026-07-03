import { test, expect, Page } from "@playwright/test";
import { createBook, registerNewUser } from "./helpers";

/**
 * Typography / font-settings persistence coverage.
 *
 * The book sidebar's "Typography" section header (`.sidebar-section-header`)
 * flips `active_pane` to `BookPane::Typography` and, ~100ms later, imperatively
 * builds the pane's controls via `setup_font_pickers` (book.rs ~3170):
 *   - the spacing grid (`#spacing-selector-grid`) gets native `<select>`s
 *     `#pw-paragraph-spacing`, `#pw-paragraph-indent`, `#pw-heading-indent`
 *     (built by `build_spacing_grid_html` from SPACING_OPTIONS / INDENT_OPTIONS);
 *   - the font grid (`#font-selector-grid`) gets `.font-picker[data-font-slot=…]`
 *     typeahead inputs backed by the `/api/fonts` (Google Fonts) catalog.
 *
 * Changing a spacing `<select>` fires its `change` listener, which updates the
 * `font_settings` signal and (after a 500ms debounce) PUTs the book via
 * `/api/books/{id}` — so the saved value round-trips both to the reloaded DOM
 * and to `GET /api/books/{id}`. Font-picker choices persist the same way.
 */

/**
 * Open the Typography pane and wait until its imperatively-built spacing select
 * exists (proving `setup_font_pickers`' deferred build ran and its change
 * listeners are attached — the selects and their handlers are wired in the same
 * synchronous closure).
 */
async function openTypographyPane(page: Page) {
  await page
    .locator(".sidebar-section-header", { hasText: "Typography" })
    .click();
  await expect(page.locator("#pw-paragraph-spacing")).toBeVisible();
}

/**
 * After a reload, wait until the book has actually loaded. `current_book` (which
 * drives the sidebar title) and the `font_settings` signal are set together in
 * the book fetch (book.rs ~2733), so a populated title guarantees the pane will
 * render the saved font settings when opened.
 */
async function waitBookLoaded(page: Page, title: string) {
  await expect(page.locator(".book-sidebar-title")).toContainText(title);
}

test("paragraph-spacing select change persists across reload", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Typography Persistence Novel");

  await openTypographyPane(page);

  // Read the real option values from the DOM and pick one that differs from the
  // current selection (default "Normal" = 8, which the app maps back to None).
  const options = await page
    .locator("#pw-paragraph-spacing option")
    .evaluateAll((opts) => opts.map((o) => (o as HTMLOptionElement).value));
  const current = await page.locator("#pw-paragraph-spacing").inputValue();
  const target = options.filter((v) => v !== current).pop();
  expect(target, "expected a spacing option distinct from the current one").toBeTruthy();

  // Change the select — this fires the `change` listener that saves font_settings.
  await page.locator("#pw-paragraph-spacing").selectOption(target!);

  // The change reached the server (500ms debounced PUT of the book).
  await expect
    .poll(async () => {
      const resp = await page.request.get(`/api/books/${bookId}`);
      const book = (await resp.json()) as {
        font_settings?: { paragraph_spacing?: number | null };
      };
      return book.font_settings?.paragraph_spacing ?? null;
    })
    .toBe(Number(target));

  // Reload: the pane must reflect the persisted value, not the default.
  await page.goto(`/book/${bookId}`);
  await waitBookLoaded(page, "Typography Persistence Novel");
  await openTypographyPane(page);
  await expect(page.locator("#pw-paragraph-spacing")).toHaveValue(target!);
});

test("body font choice via the picker persists across reload", async ({ page }) => {
  await registerNewUser(page);
  const bookId = await createBook(page, "Font Picker Novel");

  // The font-picker typeahead is driven by the server's `/api/fonts` catalog,
  // which the backend fetches live from Google Fonts. When that upstream fetch
  // is unavailable (offline test env => `/api/fonts` 503, empty catalog) the
  // dropdown has nothing to match, so this case is genuinely infeasible; gate on
  // the catalog actually containing our target font.
  const fontsResp = await page.request.get("/api/fonts");
  const catalog = fontsResp.ok()
    ? ((await fontsResp.json()) as Array<{ family: string }>)
    : [];
  const hasLora = Array.isArray(catalog) && catalog.some((f) => f.family === "Lora");
  test.skip(
    !hasLora,
    "Google Fonts catalog unavailable in this env (/api/fonts empty or 503) — typeahead has no options to select",
  );

  await openTypographyPane(page);

  // Focus the body slot's typeahead and filter down to "Lora".
  const bodyInput = page.locator(".font-picker[data-font-slot='body'] input");
  await bodyInput.click();
  await bodyInput.fill("Lora");

  // Commit the choice by clicking the matching dropdown option (its listener
  // fires on mousedown, which Playwright's click dispatches).
  const option = page.locator(
    ".font-picker[data-font-slot='body'] .font-dropdown .font-option[data-font-value='Lora']",
  );
  await expect(option).toBeVisible();
  await option.click();
  await expect(bodyInput).toHaveValue("Lora");

  // The choice reached the server (500ms debounced PUT of the book).
  await expect
    .poll(async () => {
      const resp = await page.request.get(`/api/books/${bookId}`);
      const book = (await resp.json()) as { font_settings?: { body?: string | null } };
      return book.font_settings?.body ?? null;
    })
    .toBe("Lora");

  // Reload: the picker input must show the persisted font.
  await page.goto(`/book/${bookId}`);
  await waitBookLoaded(page, "Font Picker Novel");
  await openTypographyPane(page);
  await expect(
    page.locator(".font-picker[data-font-slot='body'] input"),
  ).toHaveValue("Lora");
});
