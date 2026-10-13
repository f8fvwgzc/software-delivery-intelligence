/*
 * Every route renders its page frame without a JS exception. Against Vite
 * alone (no gateway) the data regions fall back to their ErrorState and
 * that is tolerated; against a running stack every PageHeader must show.
 */
import type { APIRequestContext } from '@playwright/test'

import { expect, test } from './fixtures'
import { listProjects, type Project } from './helpers/api'
import { expectPageFrame, isApiReachable, newApiContext, watchConsole } from './helpers/stack'

const ROUTES = [
  { path: '/', title: 'Overview' },
  { path: '/connect', title: 'Connect repository' },
  { path: '/projects', title: 'Projects' },
  { path: '/proposals', title: 'Proposals' },
  { path: '/executions', title: 'Executions' },
  { path: '/approvals', title: 'Approvals' },
  { path: '/knowledge/patterns', title: 'Patterns' },
  { path: '/knowledge/decisions', title: 'Decisions' },
  { path: '/knowledge/search', title: 'Search' },
  { path: '/policies', title: 'Policies' },
  { path: '/system', title: 'System' },
  { path: '/workspace/members', title: 'Members' },
  { path: '/workspace/audit', title: 'Audit' },
  { path: '/workspace/data', title: 'Data & privacy' },
] as const

const PROJECT_ROUTES = [
  { path: '', title: 'Understanding' },
  { path: '/architecture', title: 'Architecture' },
  { path: '/dependencies', title: 'Dependencies' },
  { path: '/security', title: 'Security' },
  { path: '/delivery', title: 'Delivery' },
  { path: '/proposals', title: 'Proposals' },
  { path: '/documents', title: 'Documents' },
  { path: '/executions', title: 'Executions' },
] as const

let api: APIRequestContext
let apiUp = false
let project: Project | null = null

test.beforeAll(async ({ playwright }) => {
  api = await newApiContext(playwright, test.info().project.use.baseURL)
  apiUp = await isApiReachable(api)
  project = apiUp ? ((await listProjects(api))[0] ?? null) : null
})

test.afterAll(async () => {
  await api?.dispose()
})

test.describe('top-level routes', () => {
  for (const route of ROUTES) {
    test(`${route.path} shows "${route.title}"`, async ({ page }) => {
      const guard = watchConsole(page)
      await page.goto(route.path)

      const frame = await expectPageFrame(page, route.title, { apiUp })
      if (frame === 'error-state') {
        test.info().annotations.push({
          type: 'note',
          description: `${route.path}: the gateway is unreachable, the page fell back to its ErrorState`,
        })
      }
      // The shell rendered around it: the sidebar navigation is in place
      // (the breadcrumb also exposes a role=link named after the page).
      await expect(page.locator('a[data-sidebar="menu-button"]').filter({ hasText: 'Overview' })).toBeVisible()

      guard.assertClean({ apiUp })
    })
  }

  test('/settings shows "Settings"', async ({ page }) => {
    const guard = watchConsole(page)
    await page.goto('/settings')

    const heading = page.getByRole('heading', { level: 1, name: 'Settings', exact: true })
    const notFound = page.getByText('This page does not exist')
    const retry = page.getByRole('button', { name: 'Try again' })
    await expect(heading.or(notFound).or(retry).first()).toBeVisible({ timeout: 20_000 })

    test.skip(await notFound.isVisible(), 'The /settings route is not in the router yet (CONTRACT.md §8, GET /settings)')
    if (apiUp) await expect(heading).toBeVisible()

    guard.assertClean({ apiUp })
  })

  test('an unknown route shows the not-found page', async ({ page }) => {
    const guard = watchConsole(page)
    await page.goto('/this/route/does/not/exist')

    await expect(page.getByText('This page does not exist')).toBeVisible()
    await expect(page.getByRole('link', { name: 'Back to overview' })).toBeVisible()

    guard.assertClean({ apiUp })
  })
})

test.describe('project routes', () => {
  for (const route of PROJECT_ROUTES) {
    test(`/projects/:id${route.path} shows "${route.title}"`, async ({ page }) => {
      test.skip(!project, 'needs a reachable stack with at least one connected project')
      const guard = watchConsole(page)
      await page.goto(`/projects/${project?.id}${route.path}`)

      await expectPageFrame(page, route.title, { apiUp: true })
      // The project band above the tabs rendered too.
      await expect(page.getByRole('heading', { level: 1, name: project?.repoFullName ?? '' })).toBeVisible()

      guard.assertClean({ apiUp: true })
    })
  }
})
