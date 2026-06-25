# PlotWeb e2e (Playwright)

Browser end-to-end tests that drive the real built SPA against the real Axum
server over a throwaway data dir.

## Run

```bash
cd e2e
npm install                 # first time only
npx playwright install chromium-headless-shell   # first time only (downloads the browser)
npx playwright test         # builds frontend+server, starts on :3000, runs specs, tears down
```

`npx playwright test` owns the whole lifecycle via the `webServer` block in
`playwright.config.ts`: it runs `scripts/run-test-server.sh`, which builds the
server (and the frontend `dist/` if missing), points `DATABASE_URL` /
`DATA_DIR` / `RHYPEDB_DATA_DIR` at a fresh `mktemp` dir, and serves on `:3000`.
Nothing touches your real `plotweb.db` or `data/`.

### Against an already-running server

If you already have a server on `:3000` (e.g. `cargo run` + a prebuilt `dist/`):

```bash
E2E_REUSE_SERVER=1 npx playwright test
```

## What's covered

- **auth.spec.ts** — login redirect when unauthenticated; register → logout →
  log back in; wrong password; unknown user.
- **books-chapters.spec.ts** — create a book; add a chapter, write content, and
  confirm it survives a full reload; **the lost-edits regression** (leaving the
  editor within the autosave debounce must still save); cross-user book access
  (IDOR) is blocked.

Selectors live in `tests/helpers.ts`. The frontend's inputs have no associated
`<label for>`, so locators target placeholders and button text.
