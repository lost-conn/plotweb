import { defineConfig, devices } from "@playwright/test";

/**
 * PlotWeb e2e config.
 *
 * Playwright owns the server lifecycle via `webServer`: it runs the launch
 * script (which builds + serves the real Axum server over a throwaway data dir),
 * waits for :3000, runs the specs, then tears it down. Set `E2E_REUSE_SERVER=1`
 * to reuse a server you started yourself.
 */
export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [["list"], ["html", { open: "never" }]],
  timeout: 30_000,
  expect: { timeout: 10_000 },

  use: {
    baseURL: "http://localhost:3000",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],

  webServer: {
    command: "bash scripts/run-test-server.sh",
    url: "http://localhost:3000/health",
    reuseExistingServer: !!process.env.E2E_REUSE_SERVER,
    timeout: 180_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
