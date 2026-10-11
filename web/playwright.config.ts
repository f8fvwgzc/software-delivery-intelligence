import { defineConfig, devices } from '@playwright/test'

/*
 * Playwright against a stack that is started outside of this config (no
 * `webServer`): `pnpm dev` with the gateway on the host, or `make up` for
 * the Docker stack behind nginx.
 *
 *   SDI_E2E_BASE_URL   where the app answers. Defaults to the Vite dev
 *                      server (http://localhost:5173); in CI, where the
 *                      stack comes from `make up`, to the nginx edge
 *                      (http://localhost).
 *   SDI_E2E_GITHUB=1   the specs that need a real GitHub account behind
 *                      GITHUB_TOKEN run; anything else skips them.
 *   SDI_E2E_REPO       repository the critical path connects, `owner/name`
 *                      (default f8fvwgzc/view-engine).
 *
 * Reports land in web/e2e-report, traces and screenshots in
 * web/test-results; both are git-ignored.
 */
const CI = Boolean(process.env.CI)
const baseURL = process.env.SDI_E2E_BASE_URL ?? (CI ? 'http://localhost' : 'http://localhost:5173')

export default defineConfig({
  testDir: './e2e',
  outputDir: './test-results',

  // One stack, shared state (policies, approvals, the GitHub project): the
  // spec files run one after the other, tests in file order.
  fullyParallel: false,
  workers: 1,
  forbidOnly: CI,
  retries: CI ? 1 : 0,

  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: [['list'], ['html', { outputFolder: 'e2e-report', open: 'never' }]],

  use: {
    baseURL,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'off',
    actionTimeout: 15_000,
    navigationTimeout: 30_000,
    colorScheme: 'light',
    locale: 'en-US',
    timezoneId: 'UTC',
  },

  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
})
