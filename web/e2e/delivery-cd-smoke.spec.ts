/*
 * Continuous delivery smoke (CONTRACT.md §22): the Pipeline tab of a project
 * and the two workspace admin pages, Registries and Secrets.
 *
 * The first three cases fulfil the §22 routes in the browser with empty
 * answers, so they hold whether or not the gateway serves those routes yet
 * and can never start a deployment: each page must render its frame and
 * its designed empty states, and the Pipeline tab must stay within its
 * first-paint requests. The last case leaves the routes alone: whatever the
 * gateway answers (data, or a 404 from a build that predates §22), every
 * region shows content or its own error with a retry, never a blank page.
 */
import type { APIRequestContext, Locator, Page } from '@playwright/test'

import { expect, test } from './fixtures'
import { listProjects, type Project } from './helpers/api'
import { isApiReachable, newApiContext, watchConsole } from './helpers/stack'

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

const json = (body: unknown) => ({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })

/** The seeded default of every workspace: GHCR, signed in with the workflow token. */
const DEFAULT_REGISTRY = {
  id: 'registry-default',
  name: 'GitHub Container Registry',
  kind: 'ghcr',
  host: 'ghcr.io',
  namespace: 'acme',
  auth: 'github_token',
  usernameSecret: null,
  passwordSecret: null,
  isDefault: true,
}

/** A project with no environment, no release and no deployment yet. */
async function mockEmptyPipeline(page: Page, projectId: string) {
  await page.route('**/api/v1/projects/*/pipeline', (route) =>
    route.fulfill(json({ projectId, stages: [], registry: DEFAULT_REGISTRY, successRate: 0, deployments30d: 0, leadTimeSeconds: 0 })),
  )
  await page.route('**/api/v1/projects/*/releases**', (route) => route.fulfill(json([])))
  await page.route('**/api/v1/projects/*/deployment-runs**', (route) => route.fulfill(json([])))
  await page.route('**/api/v1/projects/*/delivery/artifacts', (route) => route.fulfill(json({ artifacts: [], notes: [], requiredSecrets: [] })))
}

/** API paths the page asked for, so a case can say what first paint may and may not fetch. */
function recordApiPaths(page: Page): string[] {
  const paths: string[] = []
  page.on('request', (request) => {
    const { pathname } = new URL(request.url())
    if (pathname.startsWith('/api/v1/')) paths.push(pathname)
  })
  return paths
}

/** A data region is never blank: it holds its content, its empty state, or its error with a retry. */
async function expectRegionSettled(region: Locator, content: Locator) {
  await expect(region).toBeVisible({ timeout: 20_000 })
  const failed = region.getByRole('alert').filter({ has: region.page().getByRole('button', { name: 'Try again' }) })
  await expect(content.or(failed).first()).toBeVisible({ timeout: 20_000 })
}

const region = (page: Page, name: string) => page.getByRole('region', { name, exact: true })

test.describe('continuous delivery smoke', () => {
  test('the Pipeline tab renders the board frame and its empty states', async ({ page }) => {
    test.skip(!project, 'needs a reachable stack with at least one connected project')
    const guard = watchConsole(page)
    const projectId = project?.id ?? ''
    await mockEmptyPipeline(page, projectId)
    const requested = recordApiPaths(page)

    await page.goto(`/projects/${projectId}/pipeline`)

    await expect(page.getByRole('heading', { level: 1, name: 'Pipeline', exact: true })).toBeVisible({ timeout: 20_000 })
    // The tab is part of the project band and marked as the current one.
    await expect(page.getByRole('navigation', { name: 'Project sections' }).getByRole('link', { name: 'Pipeline' })).toHaveAttribute('aria-current', 'page')
    // Away from the workspace pages their sidebar section stays folded (it only exists for admins).
    const section = page.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Workspace', exact: true })
    if (await section.isVisible()) await expect(section).toHaveAttribute('aria-expanded', 'false')

    // The stat strip keeps its shape with nothing deployed, and names where images go.
    await expect(page.getByText('Default registry')).toBeVisible()
    await expect(page.getByText('ghcr.io/acme')).toBeVisible()

    const environments = region(page, 'Environments')
    await expect(environments.getByText('No environments yet')).toBeVisible()
    await expect(environments.getByText(/continuous delivery moves that exact image/)).toBeVisible()
    await expect(environments.getByRole('button', { name: 'Generate setup files' })).toBeVisible()

    await expect(region(page, 'Releases').getByText('No releases yet')).toBeVisible()
    await expect(region(page, 'Deployment history').getByText('No deployments yet')).toBeVisible()

    // First paint is the pipeline, the releases and the runs: three endpoints of its own (CONTRACT.md §21).
    // A set, not a count: the event stream connecting late refetches the same three once more.
    const own = new Set(requested.filter((path) => path.startsWith(`/api/v1/projects/${projectId}/`)))
    expect([...own].toSorted(), 'endpoints the Pipeline tab reads on first paint').toEqual(
      ['deployment-runs', 'pipeline', 'releases'].map((leaf) => `/api/v1/projects/${projectId}/${leaf}`),
    )
    // Registries and secrets wait for an editor, the setup files for the button.
    expect(requested.filter((path) => /\/(registries|secrets|artifacts)$/.test(path))).toEqual([])

    // "Generate setup files" opens the panel further down; only then are the files asked for.
    await environments.getByRole('button', { name: 'Generate setup files' }).click()
    await expect(region(page, 'Setup files').getByText('Nothing to generate yet')).toBeVisible()
    expect(requested.filter((path) => path.endsWith('/delivery/artifacts'))).toHaveLength(1)

    guard.assertClean({ apiUp: true })
  })

  test('/workspace/registries renders its frame and empty state', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    await page.route('**/api/v1/registries', (route) => (route.request().method() === 'GET' ? route.fulfill(json([])) : route.fallback()))

    await page.goto('/workspace/registries')

    await expect(page.getByRole('heading', { level: 1, name: 'Registries', exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(region(page, 'Registries').getByText('No registries')).toBeVisible()
    // The shell rendered around it.
    await expect(page.locator('a[data-sidebar="menu-button"]').filter({ hasText: 'Overview' })).toBeVisible()

    guard.assertClean({ apiUp: true })
  })

  test('/workspace/secrets renders its frame, its empty state and the write-only note', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    await page.route('**/api/v1/workspaces/*/secrets', (route) => (route.request().method() === 'GET' ? route.fulfill(json([])) : route.fallback()))

    await page.goto('/workspace/secrets')

    await expect(page.getByRole('heading', { level: 1, name: 'Secrets', exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(page.getByText(/encrypted when it is stored and never shown again/)).toBeVisible()
    await expect(region(page, 'Workspace secrets').getByText('No secrets yet')).toBeVisible()
    // On one of its pages the sidebar section is open and the current row is in view.
    const main = page.getByRole('navigation', { name: 'Main' })
    await expect(main.getByRole('button', { name: 'Workspace', exact: true })).toHaveAttribute('aria-expanded', 'true')
    await expect(main.getByRole('link', { name: 'Secrets', exact: true })).toBeInViewport()

    guard.assertClean({ apiUp: true })
  })

  test('against the running gateway every region shows content or its own error, never a blank page', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)

    if (project) {
      await page.goto(`/projects/${project.id}/pipeline`)
      await expect(page.getByRole('heading', { level: 1, name: 'Pipeline', exact: true })).toBeVisible({ timeout: 20_000 })
      const environments = region(page, 'Environments')
      await expectRegionSettled(
        environments,
        environments.getByRole('list', { name: 'Environments in promotion order' }).or(environments.getByText('No environments yet')),
      )
      const releases = region(page, 'Releases')
      await expectRegionSettled(releases, releases.getByRole('list', { name: 'Releases' }).or(releases.getByText('No releases yet')))
      const history = region(page, 'Deployment history')
      await expectRegionSettled(history, history.getByRole('table').or(history.getByText('No deployments yet')))
      // The setup files are not asked for until someone wants them.
      await expect(region(page, 'Setup files').getByRole('button', { name: 'Generate setup files' })).toBeVisible()
    }

    await page.goto('/workspace/registries')
    await expect(page.getByRole('heading', { level: 1, name: 'Registries', exact: true })).toBeVisible({ timeout: 20_000 })
    const registries = region(page, 'Registries')
    await expectRegionSettled(registries, registries.getByRole('list', { name: 'Registries' }).or(registries.getByText('No registries')))

    await page.goto('/workspace/secrets')
    await expect(page.getByRole('heading', { level: 1, name: 'Secrets', exact: true })).toBeVisible({ timeout: 20_000 })
    // Reading secret names is admin only: anyone else gets the explanation, and the list is never asked for.
    const secrets = region(page, 'Workspace secrets')
    await expectRegionSettled(
      secrets,
      secrets
        .getByRole('list', { name: 'Workspace secrets' })
        .or(secrets.getByText('No secrets yet'))
        .or(secrets.getByText('Visible to admins only')),
    )

    guard.assertClean({ apiUp: true })
  })
})
