/*
 * axe-core over the surfaces an operator lives on. Serious and critical
 * violations fail; everything else is printed and attached to the report
 * so it can be worked down without blocking the suite. Colour contrast is
 * part of the serious set, so every text token is checked at ≥ 4.5:1.
 */
import { AxeBuilder } from '@axe-core/playwright'
import type { APIRequestContext, Page } from '@playwright/test'

import { expect, test } from './fixtures'
import {
  cancelExecutionQuietly,
  createDryRun,
  createProposal,
  listProjects,
  rejectProposalQuietly,
  type Project,
} from './helpers/api'
import { isApiReachable, newApiContext, waitForSettled } from './helpers/stack'

type Violation = Awaited<ReturnType<AxeBuilder['analyze']>>['violations'][number]

const BLOCKING = new Set(['serious', 'critical'])

function describe(violations: Violation[]) {
  return violations
    .map((violation) => {
      const nodes = violation.nodes.slice(0, 5).map((node) => `    ${node.target.join(' ')}`)
      return `- [${violation.impact}] ${violation.id}: ${violation.help} (${violation.helpUrl})\n${nodes.join('\n')}`
    })
    .join('\n')
}

async function audit(page: Page, name: string) {
  await waitForSettled(page)
  // Rows fade in with a stagger; axe reads the colour mid-animation otherwise.
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        // The live dots pulse forever; only the finite enter animations are awaited.
        .filter((animation) => Number.isFinite(animation.effect?.getTiming().iterations ?? 1))
        .map((animation) => animation.finished.catch(() => undefined)),
    ),
  )
  const { violations } = await new AxeBuilder({ page }).analyze()

  const blocking = violations.filter((violation) => BLOCKING.has(violation.impact ?? ''))
  const advisory = violations.filter((violation) => !BLOCKING.has(violation.impact ?? ''))

  await test.info().attach(`axe-${name}.json`, {
    body: JSON.stringify(violations, null, 2),
    contentType: 'application/json',
  })
  if (advisory.length > 0) {
    console.log(`axe ${name}: ${advisory.length} non-blocking finding(s)\n${describe(advisory)}`)
  }
  expect(blocking, `axe ${name}: serious or critical violations\n${describe(blocking)}`).toEqual([])
}

let api: APIRequestContext
let apiUp = false
let project: Project | null = null
let anyProject: Project | null = null

test.beforeAll(async ({ playwright }) => {
  api = await newApiContext(playwright, test.info().project.use.baseURL)
  apiUp = await isApiReachable(api)
  if (!apiUp) return
  const projects = await listProjects(api)
  // Proposals need facts, so a scanned project; the list pages take any.
  project = projects.find((candidate) => candidate.status === 'ready') ?? null
  anyProject = projects[0] ?? null
})

test.afterAll(async () => {
  await api?.dispose()
})

/* Pages that need nothing but a session. */
const LIST_PAGES = [
  { path: '/', title: 'Overview' },
  { path: '/projects', title: 'Projects' },
  { path: '/executions', title: 'Executions' },
  { path: '/approvals', title: 'Approvals' },
  { path: '/reviews', title: 'Reviews' },
  { path: '/activity', title: 'Activity' },
  { path: '/knowledge/patterns', title: 'Patterns' },
  { path: '/workspace/audit', title: 'Audit' },
  { path: '/workspace/members', title: 'Members' },
] as const

for (const route of LIST_PAGES) {
  test(`${route.path} has no serious accessibility violations`, async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    await page.goto(route.path)
    await expect(page.getByRole('heading', { level: 1, name: route.title, exact: true })).toBeVisible()
    await audit(page, route.title.toLowerCase())
  })
}

/* Pages of one project. */
const PROJECT_PAGES = [
  { path: '', title: 'Understanding', needsReady: true, ready: (page: Page) => page.getByRole('heading', { level: 1, name: 'Understanding', exact: true }) },
  // The code browser has no page title of its own: the tree is its landmark.
  { path: '/code', title: 'Code', needsReady: true, ready: (page: Page) => page.getByRole('tree', { name: 'Repository files' }) },
  { path: '/pull-requests', title: 'Pull requests', needsReady: false, ready: (page: Page) => page.getByRole('heading', { level: 1, name: 'Pull requests', exact: true }) },
  { path: '/security', title: 'Security', needsReady: false, ready: (page: Page) => page.getByRole('heading', { level: 1, name: 'Security', exact: true }) },
  { path: '/diagrams', title: 'Diagrams', needsReady: false, ready: (page: Page) => page.getByRole('heading', { level: 1, name: 'Diagrams', exact: true }) },
] as const

for (const route of PROJECT_PAGES) {
  test(`project ${route.title.toLowerCase()} has no serious accessibility violations`, async ({ page }) => {
    const target = route.needsReady ? project : anyProject
    test.skip(!target, route.needsReady ? 'needs a reachable stack with a scanned project' : 'needs a reachable stack with a connected project')
    await page.goto(`/projects/${target?.id}${route.path}`)
    await expect(route.ready(page)).toBeVisible({ timeout: 20_000 })
    await audit(page, `project-${route.title.toLowerCase().replace(/\s+/g, '-')}`)
  })
}

test('proposal page has no serious accessibility violations', async ({ page }) => {
  test.skip(!project, 'needs a reachable stack with a scanned project')
  const proposal = await createProposal(api, { projectId: project?.id ?? '', kind: 'deployment' })
  try {
    await page.goto(`/proposals/${proposal.id}`)
    await expect(page.getByRole('heading', { level: 1, name: proposal.title })).toBeVisible()
    await audit(page, 'proposal')
  } finally {
    await rejectProposalQuietly(api, proposal.id)
  }
})

test('execution page has no serious accessibility violations', async ({ page }) => {
  test.skip(!project, 'needs a reachable stack with a connected project')
  const execution = await createDryRun(api, project?.id ?? '')
  try {
    await page.goto(`/executions/${execution.id}`)
    await expect(page.getByRole('heading', { level: 1, name: execution.title })).toBeVisible()
    await audit(page, 'execution')
  } finally {
    await cancelExecutionQuietly(api, execution.id)
  }
})

test('the trust document page has no serious accessibility violations', async ({ page }) => {
  test.skip(!apiUp, 'needs a reachable stack')
  await page.goto('/docs/trust')
  await expect(page.getByRole('navigation', { name: 'Trust documents' })).toBeVisible()
  await audit(page, 'trust')
})

test('the stress page has no serious accessibility violations once every scenario rendered', async ({ page }) => {
  test.skip(!apiUp, 'needs a reachable stack')
  test.setTimeout(180_000)
  await page.goto('/dev/stress')
  const notFound = page.getByText('This page does not exist')
  const heading = page.getByRole('heading', { level: 1, name: 'Stress', exact: true })
  await expect(heading.or(notFound).first()).toBeVisible({ timeout: 20_000 })
  test.skip(await notFound.isVisible(), 'the stress route only exists on the Vite dev server')
  await expect(page.locator('[data-stress-ready="true"]')).toBeVisible({ timeout: 120_000 })
  await audit(page, 'stress')
})

test('the shell at 375px: the sidebar is a sheet and nothing scrolls sideways', async ({ page }) => {
  test.skip(!apiUp, 'needs a reachable stack')
  await page.setViewportSize({ width: 375, height: 740 })
  for (const path of ['/', '/projects', '/executions', '/approvals', '/reviews']) {
    await page.goto(path)
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
    await waitForSettled(page)
    // No horizontal overflow of the document itself (tables scroll inside their own container).
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
    expect(overflow, `${path} overflows the viewport by ${overflow}px`).toBeLessThanOrEqual(0)
    // The permanent sidebar is gone; the trigger opens it as a sheet.
    await expect(page.locator('a[data-sidebar="menu-button"]').filter({ hasText: 'Overview' })).toBeHidden()
    await page.getByRole('button', { name: 'Open navigation' }).click()
    const sheet = page.getByRole('dialog')
    await expect(sheet).toBeVisible()
    await expect(sheet.locator('a[data-sidebar="menu-button"]').filter({ hasText: 'Overview' })).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(sheet).toBeHidden()
  }
  await audit(page, 'mobile-reviews')
})
