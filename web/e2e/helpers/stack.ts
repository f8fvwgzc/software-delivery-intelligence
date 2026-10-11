/*
 * Stack-level helpers: where the app is, whether the gateway answers, a
 * readiness wait for `/api/v1/system/status`, and the console guard every
 * spec uses to tell a JS exception from a failed API call.
 */
import {
  expect,
  test,
  type APIRequestContext,
  type Locator,
  type Page,
  type PlaywrightWorkerArgs,
} from '@playwright/test'

import { getHealth, getSystemStatus, poll, type SystemStatus } from './api'

export const env = {
  /** `SDI_E2E_GITHUB=1`: a real GitHub account is behind the stack. */
  github: process.env.SDI_E2E_GITHUB === '1',
  /** Repository the critical path connects, `owner/name`. */
  repo: process.env.SDI_E2E_REPO ?? 'f8fvwgzc/view-engine',
}

/** An API context for hooks (`beforeAll` has no `request` fixture). */
export function newApiContext(
  playwright: PlaywrightWorkerArgs['playwright'],
  baseURL: string | undefined,
): Promise<APIRequestContext> {
  return playwright.request.newContext({
    baseURL,
    // Proposal generation and scans take longer than Playwright's 30s default.
    timeout: 120_000,
    extraHTTPHeaders: { accept: 'application/json' },
  })
}

/** True when the gateway answers `/api/v1/health` with `{"status":"ok"}`. */
export async function isApiReachable(request: APIRequestContext): Promise<boolean> {
  try {
    return (await getHealth(request)).status === 'ok'
  } catch {
    return false
  }
}

/** Polls `/api/v1/system/status` until every backing service is serving. */
export function waitForStack(request: APIRequestContext, timeoutMs = 120_000): Promise<SystemStatus> {
  return poll(() => getSystemStatus(request), (status) => status.status === 'ok', {
    timeoutMs,
    intervalMs: 2_000,
    label: 'every backing service to be serving (GET /api/v1/system/status)',
    tolerateErrors: true,
  })
}

/* Console guard */

/**
 * Logged while a data region renders its ErrorState: the 4xx/5xx the
 * browser reports for the API call, and the `ApiError` React logs for the
 * boundary that caught it. A degraded region is a designed state, not a
 * bug in the page.
 */
const ERROR_STATE_NOISE: RegExp[] = [
  /Failed to load resource: the server responded with a status of [45]\d\d[\s\S]*\/api\//i,
  /ApiError/,
  /The above error occurred/i,
  /error boundary/i,
]

/** Logged only while the gateway is unreachable: failed fetches and the event stream that cannot connect. */
const OFFLINE_NOISE: RegExp[] = [
  /Failed to load resource/i,
  /net::ERR_/i,
  /Failed to fetch/i,
  /NetworkError/i,
  /ECONNREFUSED/i,
  /Request failed with status/i,
  /upstream_(?:unavailable|timeout)/i,
  /Service (?:unavailable|timed out)/i,
  /EventSource/i,
]

/**
 * Third-party warnings React's development build prints on every page.
 * Each entry names its source, so it can go once fixed upstream.
 */
const KNOWN_DEV_WARNINGS: RegExp[] = [
  // next-themes renders its theme bootstrap <script> inside the provider; React 19 warns about it in dev.
  /Encountered a script tag while rendering React component/,
]

export interface ConsoleGuard {
  readonly errors: string[]
  readonly exceptions: string[]
  /** No uncaught exception reached `window.onerror`. */
  assertNoExceptions(): void
  /**
   * No console error beyond what a degraded region logs (and, with the
   * gateway down, the failed calls themselves). Tolerated lines are
   * annotated on the test so they stay visible in the report.
   */
  assertClean(options: { apiUp: boolean }): void
}

export function watchConsole(page: Page): ConsoleGuard {
  const errors: string[] = []
  const exceptions: string[] = []

  page.on('console', (message) => {
    if (message.type() !== 'error') return
    const { url, lineNumber } = message.location()
    errors.push(url ? `${message.text()}  @ ${url}:${lineNumber}` : message.text())
  })
  page.on('pageerror', (error) => {
    exceptions.push(error.stack ?? error.message)
  })

  const assertNoExceptions = () => {
    expect(exceptions, 'uncaught exceptions on the page').toEqual([])
  }

  return {
    errors,
    exceptions,
    assertNoExceptions,
    assertClean({ apiUp }) {
      assertNoExceptions()
      const tolerated = [...ERROR_STATE_NOISE, ...KNOWN_DEV_WARNINGS, ...(apiUp ? [] : OFFLINE_NOISE)]
      const relevant: string[] = []
      const ignored: string[] = []
      for (const line of errors) {
        ;(tolerated.some((pattern) => pattern.test(line)) ? ignored : relevant).push(line)
      }
      if (ignored.length > 0) {
        test.info().annotations.push({
          type: 'tolerated console errors',
          description: ignored.map((line) => line.split('\n')[0]).join('\n'),
        })
      }
      expect(
        relevant,
        apiUp
          ? 'console errors (failed data regions and known dev warnings excluded)'
          : 'console errors other than failed API calls (the stack is unreachable)',
      ).toEqual([])
    },
  }
}

/* Page frame */

/**
 * The page either rendered its `PageHeader` or, with the gateway down, fell
 * back to an `ErrorState` with its retry button. Anything else is a failure.
 */
export async function expectPageFrame(
  page: Page,
  title: string,
  { apiUp }: { apiUp: boolean },
): Promise<'header' | 'error-state'> {
  const heading = page.getByRole('heading', { level: 1, name: title, exact: true })
  if (apiUp) {
    await expect(heading).toBeVisible({ timeout: 20_000 })
    return 'header'
  }
  const retry = page.getByRole('button', { name: 'Try again' })
  await expect(heading.or(retry).first()).toBeVisible({ timeout: 20_000 })
  return (await heading.isVisible()) ? 'header' : 'error-state'
}

/** Every skeleton has been replaced by content (or an empty/error state). */
export async function waitForSettled(page: Page, timeoutMs = 30_000) {
  await expect(page.locator('[data-slot="skeleton"]')).toHaveCount(0, { timeout: timeoutMs })
}

/** The status badge in a page header's eyebrow, next to the `h1`. */
export function headerBadge(page: Page, label: string | RegExp): Locator {
  const header = page.locator('h1').locator('..')
  return typeof label === 'string' ? header.getByText(label, { exact: true }) : header.getByText(label)
}
