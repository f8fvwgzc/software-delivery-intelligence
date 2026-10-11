/*
 * axe-core over the four surfaces an operator lives on. Serious and
 * critical violations fail; everything else is printed and attached to the
 * report so it can be worked down without blocking the suite.
 */
import { AxeBuilder } from '@axe-core/playwright'
import { expect, test, type APIRequestContext, type Page } from '@playwright/test'

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

test.beforeAll(async ({ playwright }) => {
  api = await newApiContext(playwright, test.info().project.use.baseURL)
  apiUp = await isApiReachable(api)
  if (!apiUp) return
  // Proposals need facts, so a scanned project.
  project = (await listProjects(api)).find((candidate) => candidate.status === 'ready') ?? null
})

test.afterAll(async () => {
  await api?.dispose()
})

test('overview has no serious accessibility violations', async ({ page }) => {
  test.skip(!apiUp, 'needs a reachable stack')
  await page.goto('/')
  await expect(page.getByRole('heading', { level: 1, name: 'Overview', exact: true })).toBeVisible()
  await audit(page, 'overview')
})

test('project understanding has no serious accessibility violations', async ({ page }) => {
  test.skip(!project, 'needs a reachable stack with a scanned project')
  await page.goto(`/projects/${project?.id}`)
  await expect(page.getByRole('heading', { level: 1, name: 'Understanding', exact: true })).toBeVisible()
  await audit(page, 'understanding')
})

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
