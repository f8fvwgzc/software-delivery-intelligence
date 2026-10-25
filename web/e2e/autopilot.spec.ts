/*
 * Pull request autopilot (CONTRACT.md §23): the review page (drafted fixes,
 * "Apply N fixes", the Merge frame), the pull requests list, the activity
 * feed, the project's Security tab, the Automation section of Settings and
 * the "Source lines to the model" control on Data & privacy.
 *
 * Most cases fulfil the §23 routes in the browser with answers typed by the
 * gateway's schema, so they hold whatever build runs behind the gateway and
 * can never draft, commit, merge or change a repository: every POST and
 * PATCH in this file ends at `page.route`. One case per page leaves the
 * routes alone: whatever the gateway answers (data, or an error from a
 * build that predates §23), every region shows content or its own error
 * with a retry, never a blank page.
 *
 * The last block needs no browser: the mapping from platform events to the
 * queries they invalidate, and the lane that keeps the expensive ones from
 * being refetched in bursts, are pure and tested as such.
 */
import { AxeBuilder } from '@axe-core/playwright'
import type { APIRequestContext, Locator, Page, Request, Route } from '@playwright/test'

import type { components } from '../src/lib/api/schema'
import { gatedFamilies } from '../src/lib/live/event-families'
import { createInvalidationCoalescer, type Family } from '../src/lib/live/invalidation-coalescer'
import { expect, test } from './fixtures'
import { listProjects, sleep, type Project } from './helpers/api'
import type { Me } from './helpers/auth'
import { isApiReachable, newApiContext, waitForSettled, watchConsole } from './helpers/stack'

type Schemas = components['schemas']
type Review = Schemas['Review']
type Finding = Schemas['Finding']
type MergeReadiness = Schemas['MergeReadiness']
type MergeBlocker = Schemas['MergeBlocker']
type MergeResult = Schemas['MergeResult']
type PullRequestSummary = Schemas['PullRequestSummary']
type PullRequestsResponse = Schemas['PullRequestsResponse']
type SecurityOverview = Schemas['SecurityOverview']
type Vulnerability = Schemas['Vulnerability']
type Settings = Schemas['Settings']
type DataControls = Schemas['DataControls']
type ErrorBody = Schemas['ErrorBody']
type Role = Schemas['Role']

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

/* Answers */

const json = (body: unknown, status = 200) => ({ status, contentType: 'application/json', body: JSON.stringify(body) })
const refusal = (message: string) => json({ code: 'failed_precondition', message } satisfies ErrorBody, 412)

/** Ids that exist nowhere: the review page is fed entirely by `page.route`. */
const REVIEW_ID = 'e2e-autopilot-review'
const PROJECT_ID = 'e2e-autopilot-project'
const PR_NUMBER = 42
const HEAD_SHA = '9f3c2ab41d07e5c8b6a1f0e4d2c3b5a697081f2e'
const NOW = '2026-10-13T09:00:00Z'

function finding(overrides: Partial<Finding> & Pick<Finding, 'id' | 'title'>): Finding {
  return {
    reviewId: REVIEW_ID,
    path: 'services/gateway/src/routes/headers.rs',
    line: 41,
    endLine: 41,
    category: 'correctness',
    rule: 'correctness.unwrap',
    severity: 'medium',
    detail: 'The value is unwrapped without a check.',
    suggestion: null,
    suggestionSource: null,
    canDraftFix: false,
    fixExecutionId: null,
    decision: 'pending',
    decidedBy: null,
    decidedAt: null,
    decisionNote: null,
    ...overrides,
  }
}

function review(overrides: Partial<Review> = {}): Review {
  const findings = overrides.findings ?? []
  return {
    id: REVIEW_ID,
    projectId: PROJECT_ID,
    projectName: 'autopilot',
    prNumber: PR_NUMBER,
    prTitle: 'Read the tenant from the request header',
    prAuthor: 'octocat',
    headSha: HEAD_SHA,
    status: 'proposed',
    reasoner: 'heuristic',
    requestedBy: 'system',
    filesReviewed: 4,
    findings,
    findingsTotal: findings.length,
    findingsAccepted: findings.filter((row) => row.decision === 'accepted').length,
    fixesReady: 0,
    fixExecutionId: null,
    mergeExecutionId: null,
    steps: [
      { key: 'context', label: 'Context', status: 'done', detail: '4 files', durationMs: 60 },
      { key: 'analyze', label: 'Analyze', status: 'done', detail: `${findings.length} findings`, durationMs: 20 },
      { key: 'findings', label: 'Findings', status: 'done', detail: 'awaiting decisions', durationMs: 0 },
      { key: 'summary', label: 'Summary', status: 'done', detail: '120 words, heuristic', durationMs: 4 },
    ],
    summary: 'Reads the tenant id from `X-Tenant` instead of the path.',
    publishedUrl: null,
    error: null,
    createdAt: NOW,
    updatedAt: NOW,
    ...overrides,
  }
}

const blocker = (code: string, message: string): MergeBlocker => ({ code, message })

function readiness(overrides: Partial<MergeReadiness> = {}): MergeReadiness {
  return {
    projectId: PROJECT_ID,
    prNumber: PR_NUMBER,
    title: 'Read the tenant from the request header',
    state: 'open',
    headSha: HEAD_SHA,
    headRef: 'tenant-header',
    baseRef: 'main',
    isFork: false,
    isDependabot: false,
    mergeable: 'mergeable',
    mergeableState: 'clean',
    behindBy: 0,
    baseProtected: false,
    checksTotal: 3,
    checksPassed: 3,
    checksFailed: 0,
    checksPending: 0,
    failedChecks: [],
    pendingChecks: [],
    approvals: 1,
    changesRequested: 0,
    allowedMethods: ['merge', 'squash', 'rebase'],
    reviewId: REVIEW_ID,
    reviewStatus: 'proposed',
    undecidedBlockingFindings: 0,
    blockers: [],
    canMerge: true,
    mergeExecutionId: null,
    checkedAt: new Date().toISOString(),
    ...overrides,
  }
}

function vulnerability(overrides: Partial<Vulnerability> & Pick<Vulnerability, 'id' | 'package' | 'severity'>): Vulnerability {
  return {
    ecosystem: 'npm',
    summary: 'Prototype pollution through a crafted payload',
    currentVersion: '4.17.20',
    fixedVersion: '4.17.21',
    vulnerableRange: '< 4.17.21',
    cveId: null,
    url: `https://github.com/advisories/${overrides.id}`,
    manifestPath: 'web/package.json',
    scope: 'runtime',
    source: 'dependabot',
    fixPullRequest: null,
    detectedAt: NOW,
    ...overrides,
  }
}

function overview(projectId: string, overrides: Partial<SecurityOverview> = {}): SecurityOverview {
  const vulnerabilities = overrides.vulnerabilities ?? []
  const order = ['critical', 'high', 'medium', 'low', 'unknown']
  return {
    projectId,
    source: 'dependabot',
    sourceNote: null,
    vulnerabilities,
    bySeverity: order
      .map((severity) => ({ severity, count: vulnerabilities.filter((row) => row.severity === severity).length }))
      .filter((row) => row.count > 0),
    truncated: false,
    alerts: 'enabled',
    securityUpdates: 'disabled',
    configPresent: false,
    configPath: null,
    configEcosystems: [],
    openDependabotPullRequests: 0,
    configureExecutionId: null,
    checkedAt: new Date().toISOString(),
    ...overrides,
  }
}

/* Routes */

interface ReviewMock {
  /** What `GET /reviews/{id}` answers; a case swaps it after a mutation. */
  review: Review
  /** What `GET .../merge-readiness` answers. */
  readiness: MergeReadiness
  /** Every readiness request the page made. */
  readinessRequests: Request[]
}

/** Feeds the review page: the review, and its pull request's merge readiness. */
async function mockReviewPage(page: Page, initial: { review: Review; readiness?: MergeReadiness }): Promise<ReviewMock> {
  const mock: ReviewMock = { review: initial.review, readiness: initial.readiness ?? readiness(), readinessRequests: [] }
  // The stream stays pending: these cases count the page's own requests, and
  // an execution another spec left running in the workspace would otherwise
  // refetch readiness through the gated lane in the middle of a count.
  await page.route((url) => url.pathname === '/api/v1/events/stream', () => {})
  await page.route(`**/api/v1/reviews/${REVIEW_ID}`, (route) => route.fulfill(json(mock.review)))
  await page.route(`**/api/v1/projects/*/pull-requests/${PR_NUMBER}/merge-readiness`, (route) => {
    mock.readinessRequests.push(route.request())
    return route.fulfill(json(mock.readiness))
  })
  return mock
}

/** The session as a member of a lower role sees it: the same answer, with the role replaced. */
async function actAs(page: Page, role: Role) {
  await page.route('**/api/v1/auth/me', async (route) => {
    const response = await route.fetch()
    const me = (await response.json()) as Me
    await route.fulfill({ response, json: { ...me, workspaces: me.workspaces.map((workspace) => ({ ...workspace, myRole: role })) } })
  })
}

/** Nothing that writes may reach the gateway from this file: a request the case did not fulfil fails it. */
async function forbidWrites(page: Page) {
  const leaked: string[] = []
  await page.route(/\/api\/v1\/(findings\/[^/]+\/draft-fix|reviews\/[^/]+\/(apply-fixes|publish)|projects\/[^/]+\/(pull-requests\/\d+\/merge|security\/dependabot))$/, (route) => {
    leaked.push(`${route.request().method()} ${new URL(route.request().url()).pathname}`)
    return route.fulfill(json({ code: 'internal', message: 'not fulfilled by the test' } satisfies ErrorBody, 500))
  })
  return leaked
}

const region = (page: Page, name: string) => page.getByRole('region', { name, exact: true })
const findingItem = (page: Page, title: string) => page.getByRole('list', { name: 'Findings' }).getByRole('listitem').filter({ hasText: title })

/** A data region is never blank: it holds its content, its empty state, or its error with a retry. */
async function expectRegionSettled(target: Locator, content: Locator) {
  await expect(target).toBeVisible({ timeout: 20_000 })
  const failed = target.getByRole('alert').filter({ has: target.page().getByRole('button', { name: 'Try again' }) })
  await expect(content.or(failed).first()).toBeVisible({ timeout: 30_000 })
}

/**
 * A control that is off says why: its wrapper takes the keyboard focus the
 * disabled button cannot, and shows the reason as a tooltip.
 */
async function expectDisabledWithReason(page: Page, button: Locator, reason: string) {
  await expect(button).toBeDisabled()
  await button.locator('xpath=ancestor::span[@tabindex="0"][1]').focus()
  await expect(page.getByText(reason).first()).toBeVisible()
  await page.keyboard.press('Escape')
}

/** axe-core: nothing serious or critical on the page as it stands (the a11y spec's bar). */
async function expectAccessible(page: Page) {
  await waitForSettled(page)
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((animation) => Number.isFinite(animation.effect?.getTiming().iterations ?? 1))
        .map((animation) => animation.finished.catch(() => undefined)),
    ),
  )
  const { violations } = await new AxeBuilder({ page }).analyze()
  const blocking = violations.filter((violation) => violation.impact === 'serious' || violation.impact === 'critical')
  expect(
    blocking.map((violation) => `${violation.id}: ${violation.nodes.slice(0, 3).map((node) => node.target.join(' ')).join(' | ')}`),
    'serious or critical axe violations',
  ).toEqual([])
}

/* The review page */

test.describe('review page: fixes', () => {
  test.beforeEach(() => {
    test.skip(!apiUp, 'needs a reachable stack (the page lives inside the signed-in shell)')
  })

  test('a suggestion says who wrote it, a fix run is linked, and a fix can be drafted', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await forbidWrites(page)
    const fromRule = finding({ id: 'f-rule', title: 'Latest tag on the base image', rule: 'docker.latest_tag', suggestion: 'FROM rust:1.83-slim', suggestionSource: 'rule' })
    const inFixRun = finding({
      id: 'f-run',
      title: 'Console output left in the handler',
      suggestion: 'tracing::debug!(tenant, "resolved");',
      suggestionSource: 'model',
      decision: 'accepted',
      fixExecutionId: 'e2e-fix-run',
    })
    const draftable = finding({ id: 'f-draft', title: 'Unwrap on a missing header', severity: 'high', canDraftFix: true })
    const mock = await mockReviewPage(page, { review: review({ findings: [fromRule, inFixRun, draftable], fixExecutionId: 'e2e-fix-run' }) })

    const drafted: Finding = { ...draftable, canDraftFix: false, suggestion: 'let tenant = headers.get("x-tenant").ok_or(Error::MissingTenant)?;', suggestionSource: 'model' }
    let release: () => void = () => undefined
    const held = new Promise<void>((resolve) => (release = resolve))
    await page.route('**/api/v1/findings/f-draft/draft-fix', async (route) => {
      // Held until the case has seen the loader inside the button.
      await held
      mock.review = { ...mock.review, findings: [fromRule, inFixRun, drafted] }
      await route.fulfill(json(drafted))
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    await expect(page.getByRole('heading', { level: 1, name: 'Read the tenant from the request header' })).toBeVisible({ timeout: 20_000 })

    await expect(findingItem(page, 'Latest tag on the base image').getByText('From the rule', { exact: true })).toBeVisible()
    const carried = findingItem(page, 'Console output left in the handler')
    await expect(carried.getByText('Drafted by the model', { exact: true })).toBeVisible()
    await expect(carried.getByRole('link', { name: 'In fix run' })).toHaveAttribute('href', '/executions/e2e-fix-run')
    // The header links to the latest fix execution of the review.
    await expect(page.getByRole('link', { name: 'Fix run', exact: true })).toHaveAttribute('href', '/executions/e2e-fix-run')

    // Only the finding the server marks can be drafted for.
    await expect(page.getByRole('button', { name: 'Draft a fix' })).toHaveCount(1)
    const target = findingItem(page, 'Unwrap on a missing header')
    const draft = target.getByRole('button', { name: 'Draft a fix' })
    await draft.click()
    // The loader is inside the button, and the button cannot be pressed twice.
    await expect(draft).toHaveAttribute('aria-busy', 'true')
    await expect(draft).toBeDisabled()
    await expect(draft.locator('[data-slot="spinner"]')).toBeVisible()
    release()

    await expect(target.getByText('Drafted by the model', { exact: true })).toBeVisible()
    await expect(target.getByText('ok_or(Error::MissingTenant)')).toBeVisible()
    await expect(target.getByRole('button', { name: 'Draft a fix' })).toHaveCount(0)
    // A draft does not decide the finding: it can still be accepted or dismissed.
    await expect(target.getByRole('button', { name: 'Accept' })).toBeVisible()

    await expectAccessible(page)
    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('a refused draft keeps the server’s sentence under the finding', async ({ page }) => {
    const guard = watchConsole(page)
    const sentence = 'Drafting a fix needs the reasoner and “Source lines to the model” set to the lines a finding points at. Both are on the Data & privacy page.'
    await mockReviewPage(page, {
      review: review({
        findings: [
          finding({ id: 'f-draft', title: 'Unwrap on a missing header', canDraftFix: true }),
          finding({ id: 'f-other', title: 'Swallowed error in the retry loop', canDraftFix: true, line: 88, endLine: 90 }),
        ],
      }),
    })
    await page.route('**/api/v1/findings/f-draft/draft-fix', (route) => route.fulfill(refusal(sentence)))

    await page.goto(`/reviews/${REVIEW_ID}`)
    const target = findingItem(page, 'Unwrap on a missing header')
    await target.getByRole('button', { name: 'Draft a fix' }).click()

    // Inline, as an alert inside that finding and nowhere else; the button stays for another try.
    await expect(target.getByRole('alert')).toHaveText(sentence)
    await expect(findingItem(page, 'Swallowed error in the retry loop').getByRole('alert')).toHaveCount(0)
    await expect(target.getByRole('button', { name: 'Draft a fix' })).toBeEnabled()
    // Not a toast: the sentence appears once on the page.
    await expect(page.getByText(sentence)).toHaveCount(1)

    guard.assertClean({ apiUp: true })
  })

  test('“Apply N fixes” says what will happen, keeps a refusal in the dialog, and links to the execution', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await forbidWrites(page)
    const first = finding({ id: 'f-1', title: 'Latest tag on the base image', suggestion: 'FROM rust:1.83-slim', suggestionSource: 'rule', decision: 'accepted', path: 'Dockerfile', line: 1, endLine: 1 })
    const second = finding({ id: 'f-2', title: 'Unwrap on a missing header', suggestion: 'let tenant = tenant?;', suggestionSource: 'model', decision: 'accepted', severity: 'high' })
    const undecided = finding({ id: 'f-3', title: 'Swallowed error in the retry loop', suggestion: 'return Err(error);', suggestionSource: 'rule' })
    const mock = await mockReviewPage(page, { review: review({ findings: [first, second, undecided], fixesReady: 2 }) })

    const posts: Request[] = []
    await page.route(`**/api/v1/reviews/${REVIEW_ID}/apply-fixes`, (route: Route) => {
      posts.push(route.request())
      if (posts.length === 1) return route.fulfill(refusal('The pull request has a newer head than this review. Review the new head first.'))
      mock.review = {
        ...mock.review,
        fixesReady: 0,
        fixExecutionId: 'e2e-fix-execution',
        findings: [{ ...first, fixExecutionId: 'e2e-fix-execution' }, { ...second, fixExecutionId: 'e2e-fix-execution' }, undecided],
      }
      return route.fulfill(json(mock.review, 201))
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    const apply = page.getByRole('button', { name: 'Apply 2 fixes' })
    await expect(apply).toBeEnabled({ timeout: 20_000 })
    await apply.click()

    const dialog = page.getByRole('alertdialog')
    await expect(dialog.getByRole('heading', { name: `Apply 2 fixes to #${PR_NUMBER}?` })).toBeVisible()
    // What will happen, before anything does: one commit on the branch, after approval, nothing force-pushed.
    await expect(dialog.getByText(/as one commit on top of\s*9f3c2ab\s*on\s*tenant-header/)).toBeVisible()
    await expect(dialog.getByText(/waits for an\s+approval first/)).toBeVisible()
    await expect(dialog.getByText(/Nothing is force-pushed/)).toBeVisible()
    // The accepted findings with a suggestion, and only those.
    const fixes = dialog.getByRole('list', { name: 'Fixes in this commit' }).getByRole('listitem')
    await expect(fixes).toHaveCount(2)
    await expect(fixes.nth(0)).toContainText('Latest tag on the base image')
    await expect(fixes.nth(0)).toContainText('Dockerfile:1')
    expect(posts, 'nothing is requested before the confirmation').toHaveLength(0)

    // Refused: the sentence stays in the dialog, which stays open.
    await dialog.getByRole('button', { name: 'Apply fixes' }).click()
    await expect(dialog.getByRole('alert')).toHaveText('The pull request has a newer head than this review. Review the new head first.')
    await expect(dialog).toBeVisible()

    // Asked again: the review now names its fix execution.
    await dialog.getByRole('button', { name: 'Apply fixes' }).click()
    await expect(dialog).toBeHidden()
    await expect(page.getByText('Fix commit requested')).toBeVisible()
    await expect(page.getByRole('link', { name: 'Fix run', exact: true })).toHaveAttribute('href', '/executions/e2e-fix-execution')
    await expect(page.getByRole('button', { name: /Apply \d+ fix/ })).toHaveCount(0)
    await expect(findingItem(page, 'Unwrap on a missing header').getByRole('link', { name: 'In fix run' })).toHaveAttribute('href', '/executions/e2e-fix-execution')
    expect(posts).toHaveLength(2)
    expect(posts[1].postData(), 'apply-fixes carries no body').toBeNull()

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })
})

test.describe('review page: the Merge frame', () => {
  test.beforeEach(() => {
    test.skip(!apiUp, 'needs a reachable stack (the page lives inside the signed-in shell)')
  })

  test('blocked: the state as tags, the failing checks by name, the blockers as sentences, and no polling', async ({ page }) => {
    const guard = watchConsole(page)
    await page.clock.install()
    const mock = await mockReviewPage(page, {
      review: review(),
      readiness: readiness({
        mergeable: 'conflicting',
        mergeableState: 'dirty',
        behindBy: 3,
        baseProtected: true,
        checksPassed: 1,
        checksFailed: 2,
        failedChecks: ['build', 'lint'],
        approvals: 0,
        changesRequested: 1,
        undecidedBlockingFindings: 1,
        canMerge: false,
        blockers: [
          blocker('conflicts', 'The branch conflicts with main. Update it on GitHub first.'),
          blocker('checks_failing', '2 checks failed: build, lint.'),
          blocker('changes_requested', 'A reviewer on GitHub requested changes.'),
          blocker('findings_undecided', '1 finding of severity high or critical is still undecided in the review.'),
        ],
      }),
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    const merge = region(page, 'Merge')
    const state = merge.getByRole('list', { name: 'State on GitHub' })
    await expect(state).toBeVisible({ timeout: 20_000 })
    for (const tag of ['2 checks failed', 'Changes requested', 'No approvals', 'Conflicts', 'Behind by 3', 'Protected base']) {
      await expect(state.getByText(tag, { exact: true })).toBeVisible()
    }
    await expect(merge.getByText('Failed: build, lint', { exact: true })).toBeVisible()

    const blockers = merge.getByRole('list', { name: 'What stands in the way' }).getByRole('listitem')
    await expect(blockers).toHaveText([
      'The branch conflicts with main. Update it on GitHub first.',
      '2 checks failed: build, lint.',
      'A reviewer on GitHub requested changes.',
      '1 finding of severity high or critical is still undecided in the review.',
    ])
    await expect(merge.getByRole('button', { name: 'Merge', exact: true })).toBeDisabled()

    // GitHub is read once for the frame on first paint; the review is the page's only other request.
    expect(mock.readinessRequests, 'readiness requests on first paint').toHaveLength(1)

    // Nothing here will change by itself, so nothing is asked again: half a minute passes without a request.
    const asked = mock.readinessRequests.length
    await page.clock.fastForward(30_000)
    await sleep(500)
    expect(mock.readinessRequests.length, 'readiness requests while blocked by something that does not move').toBe(asked)
    await expect(merge.getByText(/Asks again every 10 seconds/)).toHaveCount(0)

    // The review itself rendered around the frame.
    await expect(region(page, 'Steps')).toBeVisible()
    await expect(region(page, 'Findings')).toBeVisible()

    guard.assertClean({ apiUp: true })
  })

  test('pending checks: asked again every 10 s while on screen, and on “Check again”', async ({ page }) => {
    const guard = watchConsole(page)
    await page.clock.install()
    const mock = await mockReviewPage(page, {
      review: review(),
      readiness: readiness({
        checksPassed: 2,
        checksPending: 1,
        pendingChecks: ['e2e'],
        mergeableState: 'unstable',
        canMerge: false,
        blockers: [blocker('checks_pending', '1 check is still running: e2e.')],
      }),
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    const merge = region(page, 'Merge')
    await expect(merge.getByRole('list', { name: 'State on GitHub' }).getByText('1 check pending', { exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(merge.getByText('Still running: e2e', { exact: true })).toBeVisible()
    await expect(merge.getByText(/Asks again every 10 seconds/)).toBeVisible()
    await expect(merge.getByRole('button', { name: 'Merge', exact: true })).toBeDisabled()

    // By itself, once the interval has passed.
    const asked = mock.readinessRequests.length
    await page.clock.fastForward(10_500)
    await expect.poll(() => mock.readinessRequests.length, { message: 'readiness asked again after 10 s' }).toBe(asked + 1)

    // And on request. The checks have passed by then: the frame turns ready and stops asking.
    mock.readiness = readiness()
    await merge.getByRole('button', { name: 'Check again' }).click()
    await expect(merge.getByText('Nothing stands in the way.')).toBeVisible()
    await expect(merge.getByRole('list', { name: 'State on GitHub' }).getByText('Checks passed', { exact: true })).toBeVisible()
    const settled = mock.readinessRequests.length
    await page.clock.fastForward(30_000)
    await sleep(500)
    expect(mock.readinessRequests.length, 'readiness requests once nothing is pending').toBe(settled)

    guard.assertClean({ apiUp: true })
  })

  test('ready: the method is one the repository allows, the merge is confirmed first, then the execution is linked', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await forbidWrites(page)
    const mock = await mockReviewPage(page, { review: review(), readiness: readiness({ allowedMethods: ['squash', 'rebase'] }) })

    const posts: Request[] = []
    await page.route(`**/api/v1/projects/*/pull-requests/${PR_NUMBER}/merge`, (route) => {
      posts.push(route.request())
      const requestedOn = mock.readiness
      // What the gateway says from now on: a merge execution is open.
      mock.readiness = readiness({
        allowedMethods: ['squash', 'rebase'],
        canMerge: false,
        mergeExecutionId: 'e2e-merge-execution',
        blockers: [blocker('merge_in_progress', 'A merge of this pull request is already in progress.')],
      })
      mock.review = { ...mock.review, mergeExecutionId: 'e2e-merge-execution' }
      return route.fulfill(json({ executionId: 'e2e-merge-execution', readiness: requestedOn } satisfies MergeResult, 201))
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    const merge = region(page, 'Merge')
    await expect(merge.getByText('Nothing stands in the way.')).toBeVisible({ timeout: 20_000 })
    await expect(merge.getByRole('list', { name: 'State on GitHub' }).getByText('1 approval', { exact: true })).toBeVisible()

    // Squash is what the server would pick; "Merge commit" is not allowed here, so it is not offered.
    const method = merge.getByRole('combobox', { name: 'Merge method' })
    await expect(method).toContainText('Squash and merge')
    await method.click()
    await expect(page.getByRole('option')).toHaveText([/^Squash and merge/, /^Rebase and merge/])
    await page.getByRole('option', { name: /^Rebase and merge/ }).click()
    await expect(method).toContainText('Rebase and merge')

    await merge.getByRole('switch', { name: 'Delete branch after merge' }).click()

    const button = merge.getByRole('button', { name: 'Merge', exact: true })
    await expect(button).toBeEnabled()
    await button.click()

    const dialog = page.getByRole('alertdialog')
    await expect(dialog.getByRole('heading', { name: `Merge #${PR_NUMBER} into main?` })).toBeVisible()
    await expect(dialog.getByText(/merges exactly commit\s*9f3c2ab\s*with “Rebase and merge”/)).toBeVisible()
    await expect(dialog.getByText(/then deletes the branch\s*tenant-header/)).toBeVisible()
    await expect(dialog.getByText(/waits for an approval unless this workspace’s policy for\s*repo\.merge_pull_request\s*is\s+automatic/)).toBeVisible()
    expect(posts, 'nothing is requested before the confirmation').toHaveLength(0)

    const asked = mock.readinessRequests.length
    await dialog.getByRole('button', { name: 'Request merge' }).click()
    await expect(dialog).toBeHidden()
    expect(posts).toHaveLength(1)
    expect(posts[0].postDataJSON()).toEqual({ method: 'rebase', deleteBranch: true })

    // The link replaces the button, the blocker of the running merge is listed, and the header names the run.
    await expect(merge.getByRole('link', { name: 'Open the merge run' })).toHaveAttribute('href', '/executions/e2e-merge-execution')
    await expect(merge.getByRole('button', { name: 'Merge', exact: true })).toHaveCount(0)
    await expect(merge.getByText('A merge of this pull request is already in progress.')).toBeVisible()
    await expect(page.getByRole('link', { name: 'Merge run', exact: true })).toHaveAttribute('href', '/executions/e2e-merge-execution')
    expect(mock.readinessRequests.length, 'readiness is read again after the request').toBeGreaterThan(asked)

    await expectAccessible(page)
    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('a refused merge keeps the blocker’s sentence in the dialog; a fork has no branch to delete', async ({ page }) => {
    const guard = watchConsole(page)
    await mockReviewPage(page, { review: review(), readiness: readiness({ isFork: true, allowedMethods: ['merge'] }) })
    await page.route(`**/api/v1/projects/*/pull-requests/${PR_NUMBER}/merge`, (route) =>
      route.fulfill(refusal('The head moved since GitHub was last read. Review the new head first.')),
    )

    await page.goto(`/reviews/${REVIEW_ID}`)
    const merge = region(page, 'Merge')
    await expect(merge.getByRole('list', { name: 'State on GitHub' }).getByText('Fork', { exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(merge.getByRole('combobox', { name: 'Merge method' })).toContainText('Merge commit')
    await expect(merge.getByRole('switch', { name: 'Delete branch after merge' })).toHaveCount(0)

    await merge.getByRole('button', { name: 'Merge', exact: true }).click()
    const dialog = page.getByRole('alertdialog')
    await expect(dialog.getByText(/then deletes the branch/)).toHaveCount(0)
    await dialog.getByRole('button', { name: 'Request merge' }).click()
    await expect(dialog.getByRole('alert')).toHaveText('The head moved since GitHub was last read. Review the new head first.')
    await expect(dialog).toBeVisible()

    guard.assertClean({ apiUp: true })
  })

  test('merge requested: the execution is linked instead of the button; a closed pull request offers nothing', async ({ page }) => {
    const guard = watchConsole(page)
    const mock = await mockReviewPage(page, {
      review: review({ mergeExecutionId: 'e2e-merge-execution' }),
      readiness: readiness({
        canMerge: false,
        mergeExecutionId: 'e2e-merge-execution',
        blockers: [blocker('merge_in_progress', 'A merge of this pull request is already in progress.')],
      }),
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    const merge = region(page, 'Merge')
    await expect(merge.getByRole('link', { name: 'Open the merge run' })).toHaveAttribute('href', '/executions/e2e-merge-execution', { timeout: 20_000 })
    await expect(merge.getByRole('button', { name: 'Merge', exact: true })).toHaveCount(0)
    await expect(merge.getByRole('combobox', { name: 'Merge method' })).toHaveCount(0)

    // Merged in the meantime: the state says so, and there is nothing left to ask for.
    mock.readiness = readiness({
      state: 'merged',
      canMerge: false,
      blockers: [blocker('not_open', 'This pull request is merged.')],
    })
    await merge.getByRole('button', { name: 'Check again' }).click()
    await expect(merge.getByRole('list', { name: 'State on GitHub' }).getByText('Merged', { exact: true })).toBeVisible()
    await expect(merge.getByText('This pull request is merged.')).toBeVisible()
    await expect(merge.getByRole('button', { name: 'Merge', exact: true })).toHaveCount(0)
    await expect(merge.getByRole('link', { name: 'Open the merge run' })).toHaveCount(0)

    guard.assertClean({ apiUp: true })
  })

  test('a failing readiness shows its own error with a retry while the review renders', async ({ page }) => {
    const guard = watchConsole(page)
    await page.route(`**/api/v1/reviews/${REVIEW_ID}`, (route) =>
      route.fulfill(json(review({ findings: [finding({ id: 'f-1', title: 'Unwrap on a missing header' })] }))),
    )
    let failing = true
    await page.route(`**/api/v1/projects/*/pull-requests/${PR_NUMBER}/merge-readiness`, (route) =>
      failing ? route.fulfill(refusal('GitHub is not connected for this workspace.')) : route.fulfill(json(readiness())),
    )

    await page.goto(`/reviews/${REVIEW_ID}`)
    await expect(findingItem(page, 'Unwrap on a missing header')).toBeVisible({ timeout: 20_000 })
    const merge = region(page, 'Merge')
    await expect(merge.getByRole('alert')).toContainText('GitHub is not connected for this workspace.')

    failing = false
    await merge.getByRole('button', { name: 'Try again' }).click()
    await expect(merge.getByText('Nothing stands in the way.')).toBeVisible()

    guard.assertClean({ apiUp: true })
  })
})

/* Roles */

test.describe('roles', () => {
  test.beforeEach(() => {
    test.skip(!apiUp, 'needs a reachable stack (the page lives inside the signed-in shell)')
  })

  test('a viewer reads why each control is off instead of pressing it', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await forbidWrites(page)
    await actAs(page, 'viewer')
    await mockReviewPage(page, {
      review: review({
        fixesReady: 1,
        findings: [
          finding({ id: 'f-1', title: 'Latest tag on the base image', suggestion: 'FROM rust:1.83-slim', suggestionSource: 'rule', decision: 'accepted' }),
          finding({ id: 'f-2', title: 'Unwrap on a missing header', canDraftFix: true }),
        ],
      }),
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    await expect(page.getByRole('heading', { level: 1, name: 'Read the tenant from the request header' })).toBeVisible({ timeout: 20_000 })

    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Apply 1 fix' }), 'Applying fixes needs the approver role.')
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Draft a fix' }), 'Drafting a fix needs the member role.')
    // Deciding is a member's request as well: the buttons are there, off, with the reason.
    await expect(page.getByRole('button', { name: 'Dismiss' })).toBeDisabled()
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Accept' }), 'Deciding a finding needs the member role.')

    // The pull request is ready; the role is what is missing, and the frame says so in plain sight.
    const merge = region(page, 'Merge')
    await expect(merge.getByText('Nothing stands in the way.')).toBeVisible()
    await expect(merge.getByRole('button', { name: 'Merge', exact: true })).toBeDisabled()
    await expect(merge.getByText('Merging needs the approver role.')).toBeVisible()
    await expect(merge.getByRole('combobox', { name: 'Merge method' })).toBeDisabled()
    await expect(merge.getByRole('switch', { name: 'Delete branch after merge' })).toBeDisabled()

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('an approver can apply fixes and merge, but turning on Dependabot needs an admin', async ({ page }) => {
    test.skip(!project, 'needs a connected project')
    const projectId = project?.id ?? ''
    await actAs(page, 'approver')
    await mockReviewPage(page, {
      review: review({
        fixesReady: 1,
        findings: [finding({ id: 'f-1', title: 'Latest tag on the base image', suggestion: 'FROM rust:1.83-slim', suggestionSource: 'rule', decision: 'accepted' })],
      }),
    })
    await page.route(`**/api/v1/projects/${projectId}/security**`, (route) => route.fulfill(json(overview(projectId, { alerts: 'disabled' }))))

    await page.goto(`/reviews/${REVIEW_ID}`)
    await expect(page.getByRole('button', { name: 'Apply 1 fix' })).toBeEnabled({ timeout: 20_000 })
    await expect(region(page, 'Merge').getByRole('button', { name: 'Merge', exact: true })).toBeEnabled()

    await page.goto(`/projects/${projectId}/security`)
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Turn on Dependabot' }), 'Turning on Dependabot needs the admin role.')
    // Reading is not gated: the approver can still ask GitHub again.
    await expect(page.getByRole('button', { name: 'Refresh' })).toBeEnabled()
  })

  test('a fork cannot have fixes committed to it, and the button says so', async ({ page }) => {
    await mockReviewPage(page, {
      review: review({
        fixesReady: 1,
        findings: [finding({ id: 'f-1', title: 'Latest tag on the base image', suggestion: 'FROM rust:1.83-slim', suggestionSource: 'rule', decision: 'accepted' })],
      }),
      readiness: readiness({ isFork: true }),
    })

    await page.goto(`/reviews/${REVIEW_ID}`)
    await expect(region(page, 'Merge').getByText('Fork', { exact: true })).toBeVisible({ timeout: 20_000 })
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Apply 1 fix' }), 'This pull request comes from a fork, so its branch cannot be committed to.')
  })
})

/* The pull requests list */

test.describe('pull requests list', () => {
  test('rows say where a pull request comes from, without a request per row', async ({ page }) => {
    test.skip(!project, 'needs a reachable stack with at least one connected project')
    const guard = watchConsole(page)
    const projectId = project?.id ?? ''
    const row = (number: number, title: string, flags: Partial<PullRequestSummary> = {}): PullRequestSummary => ({
      projectId,
      number,
      title,
      author: 'octocat',
      authorAvatarUrl: '',
      state: 'open',
      baseRef: 'main',
      headRef: `branch-${number}`,
      headRepo: project?.repoFullName ?? '',
      headSha: HEAD_SHA,
      changedFiles: 2,
      additions: 10,
      deletions: 4,
      htmlUrl: `https://github.com/example/repo/pull/${number}`,
      createdAt: NOW,
      updatedAt: NOW,
      latestReviewId: null,
      isFork: false,
      isBot: false,
      isDependabot: false,
      ...flags,
    })
    const list: PullRequestsResponse = {
      syncedAt: NOW,
      pullRequests: [
        row(31, 'Bump lodash from 4.17.20 to 4.17.21', { isDependabot: true, isBot: true, author: 'dependabot[bot]' }),
        row(30, 'Fix the tenant header', { isFork: true, headRepo: 'contributor/repo' }),
        row(29, 'Update the changelog', { isBot: true, author: 'release-bot' }),
        row(28, 'Read the tenant from the request header'),
      ],
    }
    await page.route(`**/api/v1/projects/${projectId}/pull-requests?*`, (route) => route.fulfill(json(list)))
    const perRow: string[] = []
    page.on('request', (request) => {
      const { pathname } = new URL(request.url())
      if (/\/pull-requests\/\d+/.test(pathname)) perRow.push(pathname)
    })

    await page.goto(`/projects/${projectId}/pull-requests`)
    const rows = region(page, 'Pull requests').getByRole('listitem')
    await expect(rows).toHaveCount(4, { timeout: 20_000 })

    const tags = (index: number) => rows.nth(index).locator('[data-slot="tag"]').filter({ hasText: /^(Dependabot|Fork|Bot)$/ })
    // Dependabot is a bot: its row says "Dependabot", not "Bot" as well.
    await expect(tags(0)).toHaveText(['Dependabot'])
    await expect(tags(1)).toHaveText(['Fork'])
    await expect(rows.nth(1).getByTitle('The branch lives in contributor/repo')).toBeVisible()
    await expect(tags(2)).toHaveText(['Bot'])
    await expect(tags(3)).toHaveCount(0)

    await waitForSettled(page)
    expect(perRow, 'the list is a summary: no request per pull request').toEqual([])
    guard.assertClean({ apiUp: true })
  })
})

/* The activity feed */

test.describe('activity', () => {
  test('the autopilot’s actions are labelled, and whoever acted is named, never shown as an id', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    const userId = '0199c0de-0000-7000-8000-00000000e2e0'
    const entry = (id: string, action: string, actor: string, detail: string): Schemas['ReviewActivity'] => ({
      id,
      action,
      actor,
      detail,
      at: NOW,
      prNumber: PR_NUMBER,
      projectId: PROJECT_ID,
      projectName: 'autopilot',
      reviewId: REVIEW_ID,
    })
    await page.route('**/api/v1/activity**', (route) =>
      route.fulfill(
        json([
          entry('a-4', 'merge.requested', 'system', 'Merge of #42 requested by the watcher (Dependabot)'),
          entry('a-3', 'fixes.requested', userId, '2 fixes in one commit on tenant-header'),
          entry('a-2', 'fix.drafted', userId, 'Unwrap on a missing header (headers.rs:41): 1 line drafted'),
          entry('a-1', 'review.started', 'watcher', 'Started for 9f3c2ab'),
        ]),
      ),
    )
    await page.route('**/api/v1/people**', (route) =>
      route.fulfill(json([{ id: userId, login: 'octocat', name: 'The Octocat', avatarUrl: '', member: true }] satisfies Schemas['Person'][])),
    )

    await page.goto('/activity')
    const rows = page.getByRole('list', { name: 'Review activity' }).getByRole('listitem')
    await expect(rows).toHaveCount(4, { timeout: 20_000 })
    await expect(rows.nth(0)).toContainText('Merge requested')
    await expect(rows.nth(0)).toContainText('system')
    await expect(rows.nth(1)).toContainText('Fixes requested')
    await expect(rows.nth(1)).toContainText('octocat')
    await expect(rows.nth(2)).toContainText('Fix drafted')
    await expect(rows.nth(2)).toContainText('octocat')
    await expect(rows.nth(3)).toContainText('watcher')
    // The stored id is looked up, not printed.
    await expect(page.getByText(userId)).toHaveCount(0)

    guard.assertClean({ apiUp: true })
  })
})

/* The Security tab */

test.describe('security tab', () => {
  let projectId = ''

  test.beforeEach(() => {
    test.skip(!project, 'needs a reachable stack with at least one connected project')
    projectId = project?.id ?? ''
  })

  /** Serves the overview; a case reads the query string of each request and swaps the answer. */
  async function mockOverview(page: Page, initial: SecurityOverview) {
    const mock = { overview: initial, requests: [] as string[] }
    await page.route(`**/api/v1/projects/${projectId}/security**`, (route) => {
      const url = new URL(route.request().url())
      if (!url.pathname.endsWith('/security')) return route.fallback()
      mock.requests.push(url.search)
      return route.fulfill(json(mock.overview))
    })
    return mock
  }

  test('Dependabot’s alerts: the status strip, the table, the severity filter and a refresh', async ({ page }) => {
    const guard = watchConsole(page)
    const mock = await mockOverview(
      page,
      overview(projectId, {
        openDependabotPullRequests: 2,
        vulnerabilities: [
          vulnerability({ id: 'GHSA-35jh-r3h4-6jhm', package: 'lodash', severity: 'critical', cveId: 'CVE-2021-23337', fixPullRequest: 7 }),
          vulnerability({ id: 'GHSA-93q8-gq69-wqmw', package: 'ansi-regex', severity: 'high', currentVersion: '5.0.0', fixedVersion: null, scope: 'development' }),
          vulnerability({ id: 'RUSTSEC-2024-0003', package: 'h2', severity: 'low', ecosystem: 'cargo', currentVersion: null, vulnerableRange: '< 0.4.4', fixedVersion: '0.4.4', url: null, manifestPath: 'Cargo.toml' }),
        ],
      }),
    )
    const own: string[] = []
    page.on('request', (request) => {
      const { pathname } = new URL(request.url())
      if (pathname.startsWith(`/api/v1/projects/${projectId}/`)) own.push(pathname.slice(`/api/v1/projects/${projectId}/`.length))
    })

    await page.goto(`/projects/${projectId}/security`)
    await expect(page.getByRole('heading', { level: 1, name: 'Security', exact: true })).toBeVisible({ timeout: 20_000 })
    // The tab is part of the project band, after Dependencies, and marked as the current one.
    const tabs = page.getByRole('navigation', { name: 'Project sections' }).getByRole('link')
    await expect(tabs.filter({ hasText: /^Security$/ })).toHaveAttribute('aria-current', 'page')
    const names = await tabs.allInnerTexts()
    expect(names.indexOf('Security')).toBe(names.findIndex((name) => name.startsWith('Dependencies')) + 1)

    // Where the list comes from, how much is open, what Dependabot does.
    await expect(page.getByText('Dependabot alerts', { exact: true })).toBeVisible()
    await expect(page.getByText('1 critical, 1 high, 1 low')).toBeVisible()
    await expect(page.getByText('Alerts on', { exact: true })).toBeVisible()
    await expect(page.getByText('Security updates off', { exact: true })).toBeVisible()
    await expect(page.getByText('Not in the repository', { exact: true })).toBeVisible()
    await expect(page.getByRole('link', { name: '2 open Dependabot pull requests' })).toHaveAttribute('href', `/projects/${projectId}/pull-requests`)

    const panel = region(page, 'Vulnerabilities')
    const table = panel.getByRole('table')
    await expect(table.getByRole('columnheader')).toHaveText(['Severity', 'Package', 'Version', 'Advisory', 'Manifest', 'Fix'])
    const rows = table.locator('tbody tr')
    // Most severe first, as the server sent them.
    await expect(rows).toHaveCount(3)
    await expect(rows.nth(0)).toContainText('Critical')
    await expect(rows.nth(0)).toContainText('lodash')
    await expect(rows.nth(0)).toContainText('4.17.20')
    await expect(rows.nth(0)).toContainText('4.17.21')
    await expect(rows.nth(0).getByRole('link', { name: /GHSA-35jh-r3h4-6jhm/ })).toHaveAttribute('href', 'https://github.com/advisories/GHSA-35jh-r3h4-6jhm')
    await expect(rows.nth(0)).toContainText('CVE-2021-23337')
    await expect(rows.nth(0)).toContainText('web/package.json')
    await expect(rows.nth(0)).toContainText('Runtime')
    // The pull requests tab was not read here, so the fix leads there rather than to a review.
    await expect(rows.nth(0).getByRole('link', { name: 'Fix: pull request #7' })).toHaveAttribute('href', `/projects/${projectId}/pull-requests`)
    await expect(rows.nth(1)).toContainText('No fix yet')
    await expect(rows.nth(1)).toContainText('Development')
    // No page for the advisory: its id is text, not a link. No manifest version: the affected range stands in.
    await expect(rows.nth(2).getByRole('link')).toHaveCount(0)
    await expect(rows.nth(2)).toContainText('RUSTSEC-2024-0003')
    await expect(rows.nth(2)).toContainText('< 0.4.4')
    await expect(rows.nth(2)).toContainText('Cargo')

    // Severity as tags that filter.
    const filter = panel.getByRole('group', { name: 'Filter by severity' })
    await expect(filter.getByRole('button')).toHaveText(['All 3', 'Critical 1', 'High 1', 'Low 1'])
    await filter.getByRole('button', { name: 'High 1' }).click()
    await expect(filter.getByRole('button', { name: 'High 1' })).toHaveAttribute('aria-pressed', 'true')
    await expect(rows).toHaveCount(1)
    await expect(rows.first()).toContainText('ansi-regex')
    await expect(panel.getByText('1 of 3')).toBeVisible()
    await filter.getByRole('button', { name: 'All 3' }).click()
    await expect(rows).toHaveCount(3)

    // One request of its own on first paint (CONTRACT.md §21): the overview, read once.
    expect([...new Set(own)], 'endpoints the Security tab reads').toEqual(['security'])
    expect(mock.requests, 'the overview is asked for once, from the cache').toEqual([''])

    // "Refresh" asks GitHub again.
    mock.overview = { ...mock.overview, alerts: 'disabled' }
    await page.getByRole('button', { name: 'Refresh' }).click()
    await expect(page.getByText('Alerts off', { exact: true })).toBeVisible()
    expect(mock.requests).toEqual(['', '?refresh=true'])

    await expectAccessible(page)
    guard.assertClean({ apiUp: true })
  })

  test('the OSV fallback says why it is not Dependabot’s list', async ({ page }) => {
    const guard = watchConsole(page)
    const note = 'Dependabot alerts are turned off for this repository, so the exact versions in its manifests were looked up at OSV instead.'
    await mockOverview(
      page,
      overview(projectId, {
        source: 'osv',
        sourceNote: note,
        alerts: 'disabled',
        vulnerabilities: [vulnerability({ id: 'GHSA-93q8-gq69-wqmw', package: 'ansi-regex', severity: 'high', source: 'osv', scope: null, detectedAt: null })],
      }),
    )

    await page.goto(`/projects/${projectId}/security`)
    await expect(page.getByText('OSV lookup', { exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(page.getByRole('note')).toHaveText(note)
    await expect(page.getByText('Alerts off', { exact: true })).toBeVisible()
    const panel = region(page, 'Vulnerabilities')
    await expect(panel.locator('tbody tr')).toHaveCount(1)
    await expect(panel.getByText('From OSV lookup, most severe first.')).toBeVisible()
    // One severity: nothing to narrow down, so no filter.
    await expect(panel.getByRole('group', { name: 'Filter by severity' })).toHaveCount(0)

    guard.assertClean({ apiUp: true })
  })

  test('designed empty states: nothing vulnerable, and no source that answered', async ({ page }) => {
    const guard = watchConsole(page)
    const mock = await mockOverview(page, overview(projectId, { securityUpdates: 'enabled', configPresent: true, configPath: '.github/dependabot.yml', configEcosystems: ['npm', 'cargo'] }))

    await page.goto(`/projects/${projectId}/security`)
    const panel = region(page, 'Vulnerabilities')
    await expect(panel.getByText('No known vulnerabilities')).toBeVisible({ timeout: 20_000 })
    await expect(panel.getByText('Dependabot has no open alert for this repository.')).toBeVisible()
    await expect(page.getByText('None in the list')).toBeVisible()
    await expect(page.getByText('.github/dependabot.yml', { exact: true })).toBeVisible()
    await expect(page.getByText('Updates npm, Cargo')).toBeVisible()
    // Everything is on already: there is nothing to turn on, so no button offers it.
    await expect(page.getByRole('button', { name: 'Turn on Dependabot' })).toHaveCount(0)

    const note = 'The token cannot read Dependabot alerts (it needs the security_events scope), and the OSV lookup did not answer in time.'
    mock.overview = overview(projectId, { source: 'none', sourceNote: note, alerts: 'unknown', securityUpdates: 'unknown' })
    await page.getByRole('button', { name: 'Refresh' }).click()
    await expect(panel.getByText('The vulnerabilities could not be read')).toBeVisible()
    // Why, in the server's words, where the list would be.
    await expect(panel.getByText(note)).toBeVisible()
    await expect(page.getByText(note), 'the reason is said once').toHaveCount(1)
    // The same request as the header's, offered where the eye is.
    await expect(panel.getByRole('button', { name: 'Refresh' })).toBeVisible()
    await expect(page.getByText('Alerts unknown', { exact: true })).toBeVisible()
    // Not read is not the same as none: the strip does not claim zero.
    await expect(page.getByText('Could not be read', { exact: true })).toBeVisible()
    await expect(page.getByText('None in the list')).toHaveCount(0)

    guard.assertClean({ apiUp: true })
  })

  test('“Turn on Dependabot”: what is off starts ticked, what is on is locked, and the execution is linked', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await forbidWrites(page)
    const mock = await mockOverview(page, overview(projectId, { alerts: 'enabled', securityUpdates: 'disabled', configPresent: false }))
    const posts: Request[] = []
    await page.route(`**/api/v1/projects/${projectId}/security/dependabot`, (route) => {
      posts.push(route.request())
      mock.overview = { ...mock.overview, configureExecutionId: 'e2e-dependabot-execution' }
      return route.fulfill(json({ executionId: 'e2e-dependabot-execution' }, 201))
    })

    await page.goto(`/projects/${projectId}/security`)
    await page.getByRole('button', { name: 'Turn on Dependabot' }).click()

    const dialog = page.getByRole('dialog')
    await expect(dialog.getByRole('heading', { name: 'Turn on Dependabot' })).toBeVisible()
    // What will happen, before anything does.
    await expect(dialog.getByText(/Creates an execution on/)).toContainText(project?.repoFullName ?? '')
    await expect(dialog.getByText(/the first run pauses to ask\s+for that access/)).toBeVisible()

    const alerts = dialog.getByRole('checkbox', { name: 'Dependabot alerts' })
    const updates = dialog.getByRole('checkbox', { name: 'Security updates' })
    const config = dialog.getByRole('checkbox', { name: 'Add .github/dependabot.yml by pull request' })
    // Already on: ticked, locked, and it says so.
    await expect(alerts).toBeChecked()
    await expect(alerts).toBeDisabled()
    await expect(dialog.getByText('Already on', { exact: true })).toHaveCount(1)
    // Not on yet: ticked and free.
    await expect(updates).toBeChecked()
    await expect(updates).toBeEnabled()
    await expect(config).toBeChecked()
    await expect(config).toBeEnabled()

    // Nothing chosen is nothing to ask for.
    const submit = dialog.getByRole('button', { name: 'Turn on Dependabot' })
    await updates.click()
    await config.click()
    await expect(submit).toBeDisabled()
    await expect(dialog.getByText('Choose at least one.')).toBeVisible()
    await updates.click()
    await expect(submit).toBeEnabled()

    await submit.click()
    await expect(dialog).toBeHidden()
    expect(posts).toHaveLength(1)
    // Only what was ticked and is not on yet travels.
    expect(posts[0].postDataJSON()).toEqual({ securityUpdates: true })

    // While that execution is open the page links to it instead of offering the button.
    await expect(page.getByText('Dependabot is being turned on.')).toBeVisible()
    await expect(page.getByRole('link', { name: 'Open the execution' })).toHaveAttribute('href', '/executions/e2e-dependabot-execution')
    await expect(page.getByRole('button', { name: 'Turn on Dependabot' })).toHaveCount(0)

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('once the execution that turns Dependabot on is over, GitHub is asked again, once', async ({ page }) => {
    const guard = watchConsole(page)
    await page.clock.install()
    const mock = await mockOverview(page, overview(projectId, { alerts: 'disabled', configureExecutionId: 'e2e-dependabot-execution' }))

    await page.goto(`/projects/${projectId}/security`)
    await expect(page.getByRole('link', { name: 'Open the execution' })).toBeVisible({ timeout: 20_000 })
    await expect(page.getByText('Alerts off', { exact: true })).toBeVisible()
    expect(mock.requests).toEqual([''])

    // The execution ended and changed the repository's settings. The next ordinary read (here: coming back to
    // the tab once the answer is stale) says so, and the page asks GitHub itself instead of the five-minute cache.
    mock.overview = overview(projectId, { alerts: 'enabled' })
    await page.clock.fastForward(61_000)
    const tabs = page.getByRole('navigation', { name: 'Project sections' })
    await tabs.getByRole('link', { name: 'Pull requests' }).click()
    await expect(page.getByRole('heading', { level: 1, name: 'Pull requests', exact: true })).toBeVisible()
    await tabs.getByRole('link', { name: 'Security', exact: true }).click()

    await expect(page.getByText('Alerts on', { exact: true })).toBeVisible()
    await expect(page.getByRole('link', { name: 'Open the execution' })).toHaveCount(0)
    await expect.poll(() => mock.requests).toEqual(['', '', '?refresh=true'])

    // When a person's own "Refresh" is what finds the execution over, that read is already fresh:
    // no second one follows it.
    mock.overview = overview(projectId, { alerts: 'disabled', configureExecutionId: 'e2e-dependabot-execution-2' })
    await page.getByRole('button', { name: 'Refresh' }).click()
    await expect(page.getByRole('link', { name: 'Open the execution' })).toBeVisible()
    mock.overview = overview(projectId, { alerts: 'enabled' })
    await page.getByRole('button', { name: 'Refresh' }).click()
    await expect(page.getByRole('link', { name: 'Open the execution' })).toHaveCount(0)
    await sleep(1_500)
    expect(mock.requests).toEqual(['', '', '?refresh=true', '?refresh=true', '?refresh=true'])

    guard.assertClean({ apiUp: true })
  })

  test('300 vulnerabilities: only the rows on screen are in the page', async ({ page }) => {
    const guard = watchConsole(page)
    const many = Array.from({ length: 300 }, (_, index) =>
      vulnerability({ id: `GHSA-${String(index).padStart(4, '0')}-e2e0-test`, package: `package-${index}`, severity: index < 40 ? 'critical' : index < 160 ? 'high' : 'medium' }),
    )
    await mockOverview(page, overview(projectId, { vulnerabilities: many, truncated: true }))

    await page.goto(`/projects/${projectId}/security`)
    const panel = region(page, 'Vulnerabilities')
    await expect(panel.locator('table[data-virtual]')).toBeVisible({ timeout: 20_000 })
    await expect(panel.getByText('300 vulnerabilities')).toBeVisible()
    await expect(panel.getByText('More exist than were read.')).toBeVisible()
    const rendered = await panel.locator('tbody tr:not([aria-hidden])').count()
    expect(rendered, 'rows in the DOM for a list of 300').toBeGreaterThan(5)
    expect(rendered, 'rows in the DOM for a list of 300').toBeLessThan(120)
    // The last row is reachable by scrolling.
    await page.mouse.wheel(0, 100_000)
    await expect(panel.getByText('package-299', { exact: true })).toBeVisible()
    // A filter that leaves fewer than the threshold renders them all again.
    await panel.getByRole('button', { name: 'Critical 40' }).click()
    await expect(panel.locator('table[data-virtual]')).toHaveCount(0)
    await expect(panel.locator('tbody tr')).toHaveCount(40)

    guard.assertClean({ apiUp: true })
  })
})

/* Settings and data controls */

test.describe('settings: automation', () => {
  test.beforeEach(() => {
    test.skip(!apiUp, 'needs a reachable stack')
  })

  /** The real settings with the §23 fields set by the case; `PATCH` never reaches the gateway. */
  async function mockSettings(page: Page, automation: Partial<Settings['intelligence']>) {
    const mock = { settings: null as Settings | null, patches: [] as unknown[] }
    await page.route('**/api/v1/settings', async (route) => {
      if (!mock.settings) {
        const real = (await (await api.get('/api/v1/settings')).json()) as Settings
        mock.settings = { ...real, intelligence: { ...real.intelligence, ...automation } }
      }
      if (route.request().method() === 'PATCH') {
        const patch = route.request().postDataJSON() as Schemas['UpdateSettingsRequest']
        mock.patches.push(patch)
        const intelligence = Object.fromEntries(Object.entries(patch.intelligence ?? {}).filter(([, value]) => value != null))
        mock.settings = { ...mock.settings, intelligence: { ...mock.settings.intelligence, ...intelligence } }
      }
      return route.fulfill(json(mock.settings))
    })
    return mock
  }

  test('the four controls round-trip through one save, and the watcher says when it last looked', async ({ page }) => {
    const guard = watchConsole(page)
    const mock = await mockSettings(page, {
      autoReview: false,
      autoDraftFixes: false,
      autoMerge: 'off',
      mergeRequiresReview: true,
      prWatchIntervalSeconds: 60,
      prWatchLastRunAt: new Date(Date.now() - 12_000).toISOString(),
      prWatchLastError: null,
    })

    await page.goto('/settings')
    const automation = region(page, 'Automation')
    const review = automation.getByRole('switch', { name: 'Review pull requests automatically' })
    const draft = automation.getByRole('switch', { name: 'Draft fixes with the model' })
    const merge = automation.getByRole('combobox', { name: 'Merge on its own' })
    const gate = automation.getByRole('switch', { name: 'Require a review before merging' })
    const save = automation.getByRole('button', { name: 'Save automation' })

    await expect(review).not.toBeChecked({ timeout: 20_000 })
    await expect(draft).not.toBeChecked()
    await expect(merge).toContainText('Off')
    await expect(gate).toBeChecked()
    await expect(save).toBeDisabled()
    // One place for automatic reviews: the ranking section no longer carries the switch.
    await expect(region(page, 'Ranking').getByRole('switch')).toHaveCount(0)

    // Where the two prerequisites and the one switch that removes the human live.
    await expect(automation.getByRole('link', { name: 'Data & privacy' })).toHaveAttribute('href', '/workspace/data')
    await expect(automation.getByRole('link', { name: 'Policies' })).toHaveAttribute('href', '/policies')
    await expect(automation.getByText(/The merge still waits for an approval unless/)).toBeVisible()

    const watcher = automation.getByRole('status', { name: 'Pull request watcher' })
    await expect(watcher).toContainText(/Looks every 60 s, last looked 1\d seconds ago/)
    await expect(watcher).toContainText('It only looks while pull requests are reviewed automatically.')

    await review.click()
    await draft.click()
    await merge.click()
    await expect(page.getByRole('option')).toHaveText([/^Off/, /^Dependabot pull requests/])
    await page.getByRole('option', { name: /^Dependabot pull requests/ }).click()
    await gate.click()
    await expect(save).toBeEnabled()
    await save.click()

    await expect(page.getByText('Automation settings saved')).toBeVisible()
    expect(mock.patches).toEqual([{ intelligence: { autoReview: true, autoDraftFixes: true, autoMerge: 'dependabot', mergeRequiresReview: false } }])
    await expect(save).toBeDisabled()
    // With automatic reviews on, the watcher line has nothing to add.
    await expect(watcher).not.toContainText('It only looks while')

    // What was saved is what the page shows after a reload.
    await page.reload()
    await expect(review).toBeChecked({ timeout: 20_000 })
    await expect(draft).toBeChecked()
    await expect(merge).toContainText('Dependabot pull requests')
    await expect(gate).not.toBeChecked()

    await expectAccessible(page)
    guard.assertClean({ apiUp: true })
  })

  test('a watcher the operator turned off, and one whose last look failed, say so', async ({ page }) => {
    const mock = await mockSettings(page, { autoReview: true, prWatchIntervalSeconds: 0, prWatchLastRunAt: null, prWatchLastError: null })

    await page.goto('/settings')
    const watcher = region(page, 'Automation').getByRole('status', { name: 'Pull request watcher' })
    await expect(watcher).toHaveText('Turned off by the operator (SDI_PR_WATCH_SECONDS=0)', { timeout: 20_000 })

    if (mock.settings) {
      mock.settings = {
        ...mock.settings,
        intelligence: { ...mock.settings.intelligence, prWatchIntervalSeconds: 60, prWatchLastRunAt: null, prWatchLastError: 'GitHub answered 401 for the workspace token.' },
      }
    }
    await page.reload()
    await expect(watcher).toContainText('Looks every 60 s, has not looked yet', { timeout: 20_000 })
    await expect(watcher).toContainText('Its last look failed: GitHub answered 401 for the workspace token.')
  })
})

test.describe('data & privacy: source lines to the model', () => {
  test('the control says what is sent, saves one field, and the storage table has its row', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    const guard = watchConsole(page)
    let controls: DataControls | null = null
    const patches: unknown[] = []
    await page.route('**/api/v1/workspaces/*/data-controls', async (route) => {
      if (!controls) {
        const real = (await (await route.fetch()).json()) as DataControls
        controls = { ...real, reasoner: 'off', codeToModel: 'off' }
      }
      if (route.request().method() === 'PATCH') {
        const patch = route.request().postDataJSON() as Schemas['UpdateDataControlsRequest']
        patches.push(patch)
        controls = { ...controls, codeToModel: patch.codeToModel ?? controls.codeToModel }
      }
      return route.fulfill(json(controls))
    })

    await page.goto('/workspace/data')
    const panel = region(page, 'Controls')
    const group = panel.getByRole('radiogroup', { name: 'Source lines to the model' })
    const off = group.getByRole('radio', { name: 'Off' })
    const lines = group.getByRole('radio', { name: 'Lines a finding points at' })
    await expect(off).toBeChecked({ timeout: 20_000 })
    await expect(lines).not.toBeChecked()

    // Honest copy: what is sent, when it matters, and the default.
    const hint = panel.getByText(/Lets the model write a fix for a review finding/)
    await expect(hint).toContainText('at most 20 lines')
    await expect(hint).toContainText('120 lines at most')
    await expect(hint).toContainText('every line that looks like a secret replaced by <redacted>')
    await expect(hint).toContainText(/It only\s+matters while the reasoner is on, and it is off by default\./)

    await lines.click()
    await expect(lines).toBeChecked()
    // The reasoner is off in this workspace: turning the switch on sends nothing yet, and the page says so.
    await expect(hint).toContainText('The reasoner is off, so nothing is sent now.')

    await panel.getByRole('button', { name: 'Save controls' }).click()
    await expect(page.getByText('Data controls saved')).toBeVisible()
    expect(patches).toEqual([{ codeToModel: 'changed_lines' }])

    await page.reload()
    await expect(lines).toBeChecked({ timeout: 20_000 })

    // The "what we store" table names it too.
    const stored = region(page, 'What we store').getByRole('row').filter({ hasText: 'Source lines sent to the model to draft a fix' })
    await expect(stored).toContainText('Yes (off by default)')
    await expect(stored).toContainText('the request is not stored')

    await expectAccessible(page)
    guard.assertClean({ apiUp: true })
  })
})

/* Against the running gateway, with no route fulfilled */

test.describe('against the running gateway', () => {
  test('every region of the new pages shows content or its own error with a retry', async ({ page }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    test.setTimeout(120_000)
    const guard = watchConsole(page)
    // Reading only: a click that would write fails the case instead of reaching the gateway.
    const leaked = await forbidWrites(page)

    if (project) {
      await page.goto(`/projects/${project.id}/security`)
      await expect(page.getByRole('heading', { level: 1, name: 'Security', exact: true })).toBeVisible({ timeout: 20_000 })
      const vulnerabilities = region(page, 'Vulnerabilities')
      await expectRegionSettled(
        vulnerabilities,
        vulnerabilities
          .getByRole('table')
          .or(vulnerabilities.getByText('No known vulnerabilities'))
          .or(vulnerabilities.getByText('The vulnerabilities could not be read')),
      )
      // The strip keeps its four cells whatever the overview answered.
      for (const label of ['Source', 'Open vulnerabilities', 'Dependabot', 'Configuration file']) {
        await expect(page.locator('[data-slot="stat-strip"]').getByText(label, { exact: true })).toBeVisible()
      }

      await page.goto(`/projects/${project.id}/pull-requests`)
      const pullRequests = region(page, 'Pull requests')
      await expectRegionSettled(pullRequests, pullRequests.getByRole('list').or(pullRequests.getByText('Nothing to review')))
    }

    const listed = await api.get('/api/v1/reviews?limit=1')
    const reviews = listed.ok() ? ((await listed.json()) as Review[]) : []
    if (reviews[0]) {
      await page.goto(`/reviews/${reviews[0].id}`)
      // The review itself may fail to load: then the page says so, with a retry, and nothing else is asked for.
      const heading = page.getByRole('heading', { level: 1, name: reviews[0].prTitle })
      const pageError = page.getByRole('alert').filter({ has: page.getByRole('button', { name: 'Try again' }) })
      await expect(heading.or(pageError).first()).toBeVisible({ timeout: 30_000 })
      if (await heading.isVisible()) {
        // The review renders without waiting for GitHub; the Merge frame settles on its own.
        await expect(region(page, 'Steps')).toBeVisible()
        await expect(region(page, 'Findings')).toBeVisible()
        const merge = region(page, 'Merge')
        await expectRegionSettled(merge, merge.getByRole('button', { name: 'Check again' }))
      } else {
        test.info().annotations.push({ type: 'note', description: 'the gateway could not load the review: the page showed its own error with a retry' })
      }
    } else {
      test.info().annotations.push({ type: 'note', description: 'no review could be listed in this stack: the review page was only checked with fulfilled routes' })
    }

    await page.goto('/settings')
    const automation = region(page, 'Automation')
    await expectRegionSettled(automation, automation.getByRole('button', { name: 'Save automation' }))

    await page.goto('/workspace/data')
    const controls = region(page, 'Controls')
    await expectRegionSettled(controls, controls.getByRole('radiogroup', { name: 'Source lines to the model' }))

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })
})

/* Live invalidation: pure, no browser */

test.describe('live events and the gated lane', () => {
  const event = (entity: string, action: string, status: string, projectId: string | null = 'p1') => ({ entity, action, status, projectId })

  test('only an event that can change merge readiness or the security overview reaches them, by project', () => {
    const readinessOf = ['merge-readiness', 'p1']

    // A review: its start and its end move the gate; the steps in between do not.
    expect(gatedFamilies(event('review', 'created', 'context'))).toEqual([readinessOf])
    expect(gatedFamilies(event('review', 'updated', 'context'))).toEqual([])
    expect(gatedFamilies(event('review', 'updated', 'analyzing'))).toEqual([])
    expect(gatedFamilies(event('review', 'updated', 'proposed'))).toEqual([readinessOf])
    expect(gatedFamilies(event('review', 'updated', 'published'))).toEqual([readinessOf])
    expect(gatedFamilies(event('review', 'updated', 'failed'))).toEqual([readinessOf])

    // A finding: a verdict can lift the gate; a fix drafted for a pending finding cannot.
    expect(gatedFamilies(event('finding', 'updated', 'accepted'))).toEqual([readinessOf])
    expect(gatedFamilies(event('finding', 'updated', 'dismissed'))).toEqual([readinessOf])
    expect(gatedFamilies(event('finding', 'updated', 'pending'))).toEqual([])

    // An execution: when it is created and when it is over, never for the steps in between.
    const settled = [readinessOf, ['security', 'p1'], ['reviews']]
    expect(gatedFamilies(event('execution', 'created', 'planning'))).toEqual(settled)
    for (const status of ['executing', 'waiting_for_approval', 'verifying', 'paused', 'diagnosing']) {
      expect(gatedFamilies(event('execution', 'updated', status)), status).toEqual([])
    }
    for (const status of ['succeeded', 'failed', 'cancelled']) {
      expect(gatedFamilies(event('execution', 'updated', status)), status).toEqual(settled)
    }

    // Without a project the whole family is named; other entities never enter the lane.
    expect(gatedFamilies(event('execution', 'updated', 'succeeded', null))).toEqual([['merge-readiness'], ['security'], ['reviews']])
    for (const entity of ['project', 'scan', 'proposal', 'approval', 'deployment', 'release', 'environment', 'pattern']) {
      expect(gatedFamilies(event(entity, 'updated', 'succeeded')), entity).toEqual([])
    }
  })

  test('a burst of events ends in one refetch of each gated family, not one per event', async () => {
    const flushes: Family[][] = []
    const lane = createInvalidationCoalescer({ onFlush: (families) => flushes.push(families), intervalMs: 40, leading: false })

    // A merge request as the stream reports it: the review, the execution, then its steps, in a few milliseconds.
    const burst = [
      event('review', 'updated', 'proposed'),
      event('execution', 'created', 'planning'),
      event('execution', 'updated', 'executing'),
      event('execution', 'updated', 'waiting_for_approval'),
      event('finding', 'updated', 'accepted'),
    ]
    for (const item of burst) for (const family of gatedFamilies(item)) lane.add(family)

    // Nothing at the leading edge: the burst is still arriving.
    expect(flushes).toEqual([])
    await sleep(120)
    expect(flushes).toEqual([[['merge-readiness', 'p1'], ['security', 'p1'], ['reviews']]])

    // The steps of a running execution add nothing, however many there are.
    for (let step = 0; step < 50; step++) for (const family of gatedFamilies(event('execution', 'updated', 'executing'))) lane.add(family)
    await sleep(120)
    expect(flushes).toHaveLength(1)

    // Its end is one more flush.
    for (const family of gatedFamilies(event('execution', 'updated', 'succeeded'))) lane.add(family)
    await sleep(120)
    expect(flushes).toHaveLength(2)
    lane.dispose()
  })

  test('the default lane still flushes the first event at once', () => {
    const flushes: Family[][] = []
    const lane = createInvalidationCoalescer({ onFlush: (families) => flushes.push(families), intervalMs: 40 })
    lane.add(['reviews'])
    expect(flushes).toEqual([[['reviews']]])
    lane.dispose()
  })
})
