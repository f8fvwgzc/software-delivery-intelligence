/*
 * The development stress page (`/dev/stress`, src/dev/stress): every worst
 * case over synthetic data, one scenario after another. The page records
 * long tasks through a PerformanceObserver and every scenario checks
 * itself; this spec waits for all of them, then asserts no main-thread
 * task over 200 ms, no failed scenario check, no console error and no
 * uncaught exception. The route only exists on the Vite dev server, so the
 * spec skips against a production build.
 */
import type { Page } from '@playwright/test'

import { expect, test } from './fixtures'
import { isApiReachable, newApiContext, watchConsole } from './helpers/stack'

const LONG_TASK_FAIL_MS = 200
const SCENARIOS = ['projects', 'tree', 'file', 'events', 'findings', 'diagram', 'sse', 'coalescer', 'errors'] as const

interface LongTask {
  startTime: number
  duration: number
}

interface DevPerf {
  longTasks: LongTask[]
  navigations: { pathname: string; urls: string[] }[]
}

function readPerf(page: Page) {
  return page.evaluate(() => {
    const perf = (window as unknown as { sdiDevPerf?: DevPerf }).sdiDevPerf
    return perf ? { longTasks: perf.longTasks, navigations: perf.navigations } : null
  })
}

let apiUp = false

test.beforeAll(async ({ playwright }) => {
  const api = await newApiContext(playwright, test.info().project.use.baseURL)
  apiUp = await isApiReachable(api)
  await api.dispose()
})

test.describe('stress page', () => {
  test.setTimeout(180_000)

  test('every worst case renders progressively without a long task or a console error', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack (the page lives inside the signed-in shell)')
    const guard = watchConsole(page)

    await page.goto('/dev/stress')
    const notFound = page.getByText('This page does not exist')
    const heading = page.getByRole('heading', { level: 1, name: 'Stress', exact: true })
    await expect(heading.or(notFound).first()).toBeVisible({ timeout: 20_000 })
    test.skip(await notFound.isVisible(), 'the stress route only exists on the Vite dev server')

    const readout = page.locator('[data-stress-readout]')
    await expect(readout).toBeVisible()

    // Scenarios mount one after another; each one reports when it has its numbers.
    for (const id of SCENARIOS) {
      const frame = page.locator(`[data-scenario="${id}"]`)
      await expect(frame, `${id} mounted`).toBeVisible({ timeout: 30_000 })
      await expect(frame, `${id} reported`).toHaveAttribute('data-ready', 'true', { timeout: 30_000 })
    }
    await expect(page.locator('[data-stress-ready="true"]')).toBeVisible()

    // Every scenario's own checks hold (windowing, backoff, coalescing, layout time).
    await expect(readout).toHaveAttribute('data-checks-failed', '0')
    for (const id of SCENARIOS) {
      const frame = page.locator(`[data-scenario="${id}"]`)
      await expect(frame.getByText('check failed'), `${id} checks`).toHaveCount(0)
      const mountMs = Number(await frame.getAttribute('data-mount-ms'))
      test.info().annotations.push({ type: 'mount', description: `${id}: ${mountMs} ms to first commit` })
    }

    // The windowed lists really are windowed.
    expect(await page.locator('[data-scenario="projects"] tbody tr[data-index]').count()).toBeLessThan(100)
    expect(await page.locator('[data-scenario="file"] pre > div').count()).toBeLessThan(200)
    expect(await page.locator('[data-scenario="events"] ol > li').count()).toBeLessThan(120)
    expect(await page.locator('[data-scenario="findings"] ol > li').count()).toBeLessThan(100)
    expect(await page.locator('[data-scenario="tree"] [role="treeitem"]').count()).toBeLessThan(200)

    // The degraded panels show their copy and a retry that recovers.
    const gateway = page.locator('[data-error-panel="502"]')
    await expect(gateway.getByText('Service unavailable')).toBeVisible()
    await gateway.getByRole('button', { name: 'Try again' }).click()
    await expect(gateway.getByText('recovered')).toBeVisible()
    const offline = page.locator('[data-error-panel="offline"]')
    await expect(offline.getByText('Could not reach the server')).toBeVisible()
    await offline.getByRole('button', { name: 'Try again' }).click()
    await expect(offline.getByText('recovered')).toBeVisible()
    await expect(page.locator('[data-error-panel="slow"]').getByText('loaded')).toBeVisible({ timeout: 10_000 })

    // No main-thread task over the budget since the page mounted.
    const perf = await readPerf(page)
    expect(perf, 'the dev perf helper is installed').not.toBeNull()
    const navigation = perf?.navigations.findLast((entry) => entry.pathname === '/dev/stress')
    const since = navigation ? perf!.longTasks : []
    const over = since.filter((task) => task.duration > LONG_TASK_FAIL_MS)
    test.info().annotations.push({
      type: 'long tasks',
      description: `${since.length} over 50 ms, longest ${Math.max(0, ...since.map((task) => task.duration))} ms`,
    })
    expect(over, `long tasks over ${LONG_TASK_FAIL_MS} ms: ${JSON.stringify(over)}`).toEqual([])
    await expect(readout).toHaveAttribute('data-long-tasks-over', '0')

    // The page issued no API request of its own on first paint (synthetic data only);
    // the shell's own deferred calls (the sidebar's GitHub avatar) are not the page's.
    const SHELL_REQUESTS = [/^\/api\/v1\/auth\/me/, /^\/api\/v1\/system\/status/, /^\/api\/v1\/github\/account/, /^\/api\/v1\/approvals\?status=pending/]
    const pageRequests = (navigation?.urls ?? []).filter((url) => !SHELL_REQUESTS.some((pattern) => pattern.test(url)))
    expect(pageRequests).toEqual([])

    guard.assertClean({ apiUp: true })
  })
})
