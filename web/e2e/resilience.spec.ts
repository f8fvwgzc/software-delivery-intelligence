/*
 * The gateway misbehaving, as the browser sees it: a 502 from the edge, a
 * dropped connection, a 401 in the middle of a session, a slow answer. The
 * routes are intercepted per test, so the stack itself stays healthy; each
 * case must show a real page frame, explain itself and offer a way out.
 */
import type { APIRequestContext } from '@playwright/test'

import { expect, test } from './fixtures'
import { isApiReachable, newApiContext, watchConsole } from './helpers/stack'

let api: APIRequestContext
let apiUp = false

test.beforeAll(async ({ playwright }) => {
  api = await newApiContext(playwright, test.info().project.use.baseURL)
  apiUp = await isApiReachable(api)
})

test.afterAll(async () => {
  await api?.dispose()
})

test.describe('degraded gateway', () => {
  test('a 502 from the edge renders the page frame with a retry that recovers', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    let failures = 0
    await page.route('**/api/v1/projects', async (route) => {
      if (route.request().method() !== 'GET') return route.fallback()
      failures += 1
      await route.fulfill({ status: 502, contentType: 'application/json', body: JSON.stringify({ code: 'upstream_unavailable', message: 'No gateway replica answered' }) })
    })

    await page.goto('/projects')
    // The shell and the page header render; only the data region is degraded.
    await expect(page.getByRole('heading', { level: 1, name: 'Projects', exact: true })).toBeVisible()
    const alert = page.getByRole('alert').filter({ hasText: 'Service unavailable' })
    await expect(alert).toBeVisible({ timeout: 20_000 })
    await expect(alert.getByRole('button', { name: 'Try again' })).toBeVisible()
    expect(failures).toBeGreaterThanOrEqual(1)

    await page.unroute('**/api/v1/projects')
    await alert.getByRole('button', { name: 'Try again' }).click()
    const table = page.getByRole('table')
    const empty = page.getByText('No repositories connected')
    await expect(table.or(empty).first()).toBeVisible({ timeout: 20_000 })
    await expect(page.getByRole('alert')).toHaveCount(0)

    guard.assertClean({ apiUp: true })
  })

  test('a dropped connection shows the offline copy and recovers on retry', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    await page.route('**/api/v1/executions**', (route) => (route.request().method() === 'GET' ? route.abort('connectionfailed') : route.fallback()))

    await page.goto('/executions')
    await expect(page.getByRole('heading', { level: 1, name: 'Executions', exact: true })).toBeVisible()
    const alert = page.getByRole('alert').filter({ hasText: 'Could not reach the server' })
    await expect(alert).toBeVisible({ timeout: 20_000 })

    await page.unroute('**/api/v1/executions**')
    await alert.getByRole('button', { name: 'Try again' }).click()
    await expect(page.getByRole('table').or(page.getByText('No executions yet')).first()).toBeVisible({ timeout: 20_000 })

    // The dropped calls themselves are logged by the browser; nothing else may be.
    guard.assertClean({ apiUp: false })
  })

  test('going offline shows the banner; cached pages still render; coming back clears it', async ({ page, context }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    // Visit the page once (client-side, so its chunk stays in this document); offline, only loaded routes can render.
    await page.goto('/')
    await expect(page.getByRole('heading', { level: 1, name: 'Overview', exact: true })).toBeVisible()
    await page.getByRole('link', { name: 'Approvals', exact: true }).click()
    await expect(page.getByRole('heading', { level: 1, name: 'Approvals', exact: true })).toBeVisible()
    await page.getByRole('link', { name: 'Overview', exact: true }).click()
    await expect(page.getByRole('heading', { level: 1, name: 'Overview', exact: true })).toBeVisible()

    await context.setOffline(true)
    await expect(page.getByRole('status').filter({ hasText: 'You are offline' })).toBeVisible()
    // A navigation while offline: the page frame paints and the region shows its cached rows (or, with nothing cached, its offline copy).
    await page.getByRole('link', { name: 'Approvals', exact: true }).click()
    await expect(page.getByRole('heading', { level: 1, name: 'Approvals', exact: true })).toBeVisible()
    const region = page
      .getByRole('list', { name: 'Pending approvals' })
      .or(page.getByText('Nothing waiting on you'))
      .or(page.getByRole('alert').filter({ hasText: 'You are offline' }))
    await expect(region.first()).toBeVisible({ timeout: 20_000 })

    await context.setOffline(false)
    await expect(page.getByRole('status').filter({ hasText: 'You are offline' })).toHaveCount(0)
    // Whatever failed while offline is refetched by itself when the connection returns.
    await expect(page.getByRole('alert')).toHaveCount(0, { timeout: 20_000 })
    await expect(page.getByRole('list', { name: 'Pending approvals' }).or(page.getByText('Nothing waiting on you')).first()).toBeVisible({ timeout: 20_000 })

    // The calls that failed while offline are logged by the browser; nothing else may be.
    guard.assertClean({ apiUp: false })
  })

  test('a 401 in the middle of a session sends the browser to the login page with returnTo', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    await page.goto('/')
    await expect(page.getByRole('heading', { level: 1, name: 'Overview', exact: true })).toBeVisible()

    // The session is gone: the next page's data call answers 401.
    await page.route('**/api/v1/proposals**', (route) =>
      route.fulfill({ status: 401, contentType: 'application/json', body: JSON.stringify({ code: 'unauthenticated', message: 'Session expired' }) }),
    )
    await page.getByRole('link', { name: 'Proposals', exact: true }).click()
    await expect(page).toHaveURL(/\/login\?returnTo=%2Fproposals/, { timeout: 20_000 })
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in with GitHub' })).toBeVisible()
  })

  test('a slow gateway shows skeletons, then the data, never a blank page', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    const DELAY_MS = 2_500
    await page.route('**/api/v1/projects', async (route) => {
      if (route.request().method() !== 'GET') return route.fallback()
      await new Promise((resolve) => setTimeout(resolve, DELAY_MS))
      await route.continue()
    })

    const started = Date.now()
    await page.goto('/projects')
    await expect(page.getByRole('heading', { level: 1, name: 'Projects', exact: true })).toBeVisible()
    // The skeleton is on screen while the answer is in flight.
    await expect(page.locator('[data-slot="skeleton"]').first()).toBeVisible()
    expect(Date.now() - started).toBeLessThan(DELAY_MS)
    await expect(page.getByRole('table').or(page.getByText('No repositories connected')).first()).toBeVisible({ timeout: 20_000 })
    await expect(page.locator('[data-slot="skeleton"]')).toHaveCount(0)

    guard.assertClean({ apiUp: true })
  })
})
