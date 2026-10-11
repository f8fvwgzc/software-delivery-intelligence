/*
 * CONTRACT.md §11: connect → scan (live) → proposal → negotiate → accept →
 * dry run with an injected fault → resolve → succeeded, driven through the
 * browser. Needs SDI_E2E_GITHUB=1 and a stack whose knowledge service has a
 * GITHUB_TOKEN that can see SDI_E2E_REPO (default f8fvwgzc/view-engine).
 * The project is reused when it is already connected and never deleted.
 */
import { expect, test, type APIRequestContext, type Locator, type Page } from '@playwright/test'

import { testIds } from '../src/test-ids'
import {
  cancelExecutionQuietly,
  getExecution,
  getGithubAccount,
  getMaturity,
  getPattern,
  getProject,
  getProposal,
  getScan,
  isPausedOnFailure,
  isTerminal,
  listEvents,
  listFacts,
  poll,
  rejectProposalQuietly,
  type Execution,
} from './helpers/api'
import { env, headerBadge, newApiContext, waitForStack, watchConsole } from './helpers/stack'

const QUESTION = 'Can we use something cheaper than Kubernetes?'
const FAULT_CODE = 'SKU_UNAVAILABLE'
const SCAN_TIMEOUT = 90_000

/** Picks the dry-run adapter when the accept dialog offers a choice; `simulated` is the default otherwise. */
async function chooseDryRunIfOffered(page: Page, dialog: Locator) {
  const radio = dialog.getByRole('radio', { name: /dry run/i })
  if ((await radio.count()) > 0) {
    await radio.first().click()
    return
  }
  const select = dialog.getByRole('combobox', {
    name: /adapter|runtime|run (?:as|with)/i,
  })
  if ((await select.count()) > 0) {
    await select.first().click()
    const option = page.getByRole('option', { name: /dry run|simulated/i })
    if ((await option.count()) > 0) await option.first().click()
    else await page.keyboard.press('Escape')
  }
}

/** Approves the step that is waiting on the execution rail, if it is still waiting. */
async function approveInUi(page: Page) {
  const approve = page.getByRole('list', { name: 'Execution steps' }).getByRole('button', { name: 'Approve' }).first()
  try {
    await approve.click({ timeout: 10_000 })
  } catch {
    return // the step moved on before the control rendered; the loop re-reads the status
  }
  await expect(page.getByText(/^Approved "/).first())
    .toBeVisible({ timeout: 10_000 })
    .catch(() => undefined)
}

/**
 * Picks the recommended remediation in the failure panel and continues.
 * A dry run pauses more than once: after the injected fault, a plan step
 * whose permission the simulated identity lacks stops with PERMISSION_DENIED
 * and the recommended way out is `request_access` (CONTRACT.md §5.5).
 */
async function resolveRecommendedInUi(page: Page) {
  const panel = page.getByTestId(testIds.execution.failurePanel)
  await expect(panel).toBeVisible({ timeout: 15_000 })
  await expect
    .poll(() => panel.getByRole('radio').count(), {
      message: 'remediation options are listed',
    })
    .toBeGreaterThanOrEqual(2)
  const recommended = panel
    .locator('label')
    .filter({ hasText: /Recommended/ })
    .getByRole('radio')
  await recommended.click()
  await expect(recommended).toBeChecked()
  await panel.getByRole('button', { name: 'Continue' }).click()
  // Two resolutions in a row can show two toasts; the newest is the one that counts.
  await expect(page.getByText('Replanning from the last checkpoint').last()).toBeVisible()
}

/**
 * Re-reads the execution until `until` holds, approving in the UI whenever
 * a step waits for a decision and, with `resolve`, taking the recommended
 * remediation whenever it pauses on a failure. Fails when the run ends first.
 */
async function driveUntil(
  page: Page,
  api: APIRequestContext,
  executionId: string,
  until: (execution: Execution) => boolean,
  { timeoutMs, label, resolve = false }: { timeoutMs: number; label: string; resolve?: boolean },
): Promise<Execution> {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const execution = await getExecution(api, executionId)
    if (until(execution)) return execution
    if (isTerminal(execution.status)) {
      throw new Error(`Execution #${execution.number} reached ${execution.status} while waiting for ${label}`)
    }
    if (Date.now() >= deadline) {
      throw new Error(`Timed out after ${timeoutMs}ms waiting for ${label}; the execution is ${execution.status}`)
    }
    if (execution.status === 'waiting_for_approval') await approveInUi(page)
    else if (resolve && isPausedOnFailure(execution.status) && execution.failure) await resolveRecommendedInUi(page)
    else await page.waitForTimeout(1_000)
  }
}

test.describe('critical path', () => {
  test.skip(!env.github, 'Set SDI_E2E_GITHUB=1 and run against a stack whose knowledge service has GITHUB_TOKEN')
  test.setTimeout(12 * 60_000)

  let api: APIRequestContext
  const created: { proposalId?: string; executionId?: string } = {}

  test.beforeAll(async ({ playwright }) => {
    api = await newApiContext(playwright, test.info().project.use.baseURL)
    await waitForStack(api)
    const account = await getGithubAccount(api)
    if (!account.tokenConfigured) {
      throw new Error('GITHUB_TOKEN is not configured on the knowledge service; the critical path needs a real account')
    }
  })

  test.afterAll(async () => {
    // A run that stopped halfway leaves nothing live behind.
    if (created.executionId) await cancelExecutionQuietly(api, created.executionId)
    if (created.proposalId) await rejectProposalQuietly(api, created.proposalId)
    await api?.dispose()
  })

  test('connect → scan → proposal → negotiate → accept → dry run with a fault → resolve → succeeded', async ({
    page,
  }) => {
    const guard = watchConsole(page)

    const projectId = await test.step(`Connect ${env.repo}, or open it when it is already connected`, async () => {
      await page.goto('/connect')
      await expect(
        page.getByRole('heading', {
          level: 1,
          name: 'Connect repository',
          exact: true,
        }),
      ).toBeVisible()

      await page.getByLabel('Filter repositories by name').fill(env.repo.split('/')[1] ?? env.repo)
      const row = page.getByRole('row').filter({ hasText: env.repo }).first()
      await expect(row, `${env.repo} is listed for the configured token`).toBeVisible({ timeout: 30_000 })

      const open = row.getByRole('link', { name: 'Open' })
      if (await open.isVisible()) await open.click()
      else await row.getByRole('button', { name: 'Connect' }).click()

      await page.waitForURL(/\/projects\/[^/?#]+\/?$/, { timeout: 30_000 })
      const id = /\/projects\/([^/?#]+)/.exec(page.url())?.[1]
      if (!id) throw new Error(`Unexpected URL after connecting: ${page.url()}`)
      return id
    })

    const project = await test.step('Watch the scan on the live rail until the project is ready', async () => {
      const before = await getProject(api, projectId)
      if (before.status === 'ready') {
        // Reused and already scanned: rescan so the live rail is exercised too.
        await page.getByRole('button', { name: 'Rescan' }).click()
      }
      await expect(page.getByTestId(testIds.understanding.scanRail)).toBeVisible({ timeout: 30_000 })

      try {
        await expect(headerBadge(page, 'Ready')).toBeVisible({
          timeout: SCAN_TIMEOUT,
        })
      } catch (error) {
        const current = await getProject(api, projectId)
        const scan = current.lastScanId ? await getScan(api, current.lastScanId) : null
        throw new Error(
          `The project did not become ready within ${SCAN_TIMEOUT}ms: status ${current.status}, scan ${scan?.status ?? 'none'}${scan?.error ? ` (${scan.error})` : ''}`,
          { cause: error },
        )
      }
      return getProject(api, projectId)
    })

    await test.step('Understanding shows a TL;DR, at least five detected facts and a maturity score', async () => {
      const tldr = page.getByTestId(testIds.understanding.tldr)
      await expect(tldr).toBeVisible({ timeout: 30_000 })
      await expect(tldr).not.toContainText('No summary was generated')

      const facts = page.getByTestId(testIds.understanding.detectedFact)
      await expect(facts.nth(4), 'at least five detected facts are listed').toBeVisible({ timeout: 30_000 })
      expect((await listFacts(api, project.id, 'detected')).length).toBeGreaterThanOrEqual(5)

      const maturity = await getMaturity(api, project.id)
      expect(maturity.overall).toBeGreaterThanOrEqual(0)
      expect(maturity.overall).toBeLessThanOrEqual(100)
      expect(['legacy', 'transitional', 'modern']).toContain(maturity.stage)
      expect(maturity.axes).toHaveLength(8)

      const score = page.getByTestId(testIds.understanding.maturityScore)
      await expect(score, 'the delivery maturity score is rendered').toBeVisible({ timeout: 30_000 })
      await expect(score).toHaveText(String(maturity.overall))
    })

    const proposalId = await test.step('Generate a deployment proposal', async () => {
      await page.goto(`/projects/${project.id}/proposals`)
      await page.getByRole('button', { name: 'New proposal' }).first().click()

      const dialog = page.getByRole('dialog', { name: 'New proposal' })
      await expect(dialog.getByRole('combobox', { name: 'Kind' })).toContainText('Deployment')
      await dialog.getByRole('button', { name: 'Generate proposal' }).click()

      await page.waitForURL(/\/proposals\/[^/?#]+\/?$/, { timeout: 120_000 })
      const id = /\/proposals\/([^/?#]+)/.exec(page.url())?.[1]
      if (!id) throw new Error(`Unexpected URL after generating the proposal: ${page.url()}`)
      created.proposalId = id
      return id
    })

    await test.step('The recommended option is shown with its evidence', async () => {
      const hero = page.getByRole('article').filter({ hasText: 'Recommended option' })
      await expect(hero).toBeVisible()
      await expect(hero.getByRole('heading', { level: 2 })).not.toBeEmpty()
      await expect(hero.getByText('overall score')).toBeVisible()
      await expect(hero.getByRole('heading', { name: /^Evidence/ })).toHaveText(/Evidence \([1-9]\d*\)/)
      await expect(
        hero
          .locator('section')
          .filter({ hasText: /^Evidence/ })
          .getByRole('listitem')
          .first(),
      ).toBeVisible()
    })

    await test.step(`Negotiate: "${QUESTION}"`, async () => {
      const panel = page.getByRole('complementary', { name: 'Negotiation' })
      const replies = panel.locator('li').filter({ hasText: /Assistant ·/ })
      const before = await replies.count()

      await panel.getByLabel('Your message').fill(QUESTION)
      await panel.getByRole('button', { name: 'Send' }).click()

      await expect
        .poll(() => replies.count(), {
          timeout: 120_000,
          message: 'an assistant reply arrives',
        })
        .toBeGreaterThan(before)
      const reply = replies.last()
      const evidence = reply
        .getByRole('list', { name: 'References' })
        .or(reply.getByText(/evidence/i))
        .first()
      await expect(evidence, 'the reply cites evidence').toBeVisible()

      await expect(page.getByTestId(testIds.proposal.constraint).first(), 'a constraint chip appears').toBeVisible({
        timeout: 30_000,
      })
      expect((await getProposal(api, proposalId)).constraints.length).toBeGreaterThan(0)
    })

    const executionId = await test.step('Accept as a dry run and create the execution', async () => {
      await page.getByRole('button', { name: 'Accept', exact: true }).click()
      const dialog = page.getByRole('dialog', { name: 'Accept proposal' })
      await expect(dialog).toBeVisible()

      await chooseDryRunIfOffered(page, dialog)
      const create = dialog.getByRole('checkbox', {
        name: 'Create execution now',
      })
      if (!(await create.isChecked())) await create.click()
      await dialog.getByRole('button', { name: 'Accept option' }).click()
      await expect(page.getByText('Proposal accepted').first()).toBeVisible()

      const accepted = await poll(
        () => getProposal(api, proposalId),
        (current) => Boolean(current.executionId),
        {
          timeoutMs: 30_000,
          label: 'the accepted proposal to carry an execution id',
        },
      )
      const id = accepted.executionId as string
      created.executionId = id

      const open = page.getByRole('link', { name: 'Open execution' })
      const linked = await open.waitFor({ state: 'visible', timeout: 10_000 }).then(
        () => true,
        () => false,
      )
      if (linked) await open.click()
      else await page.goto(`/executions/${id}`)
      await page.waitForURL(new RegExp(`/executions/${id}/?$`))
      return id
    })

    const planned = await test.step(`Start with ${FAULT_CODE} injected on the second step`, async () => {
      const execution = await getExecution(api, executionId)
      const steps = [...execution.steps].sort((a, b) => a.position - b.position)
      const faultStep = steps[1]
      if (!faultStep) throw new Error(`The plan has ${steps.length} step(s); the fault needs a second one`)
      expect(execution.adapter).toBe('simulated')

      await expect(headerBadge(page, 'Planning')).toBeVisible()
      await page.getByRole('button', { name: 'Start', exact: true }).click()
      const dialog = page.getByRole('dialog', { name: /^Start execution #/ })
      await expect(dialog).toBeVisible()

      await dialog.getByRole('switch', { name: 'Inject a fault' }).click()
      await dialog.getByRole('combobox', { name: 'Step' }).click()
      await page.getByRole('option').filter({ hasText: faultStep.key }).first().click()
      await dialog.getByRole('combobox', { name: 'Failure code' }).click()
      await page.getByRole('option', { name: FAULT_CODE, exact: true }).click()
      await dialog.getByRole('button', { name: 'Start with fault' }).click()

      await expect(page.getByText(/^Execution #\d+ started$/)).toBeVisible()
      return { execution, faultStep }
    })

    await test.step('Approve what asks, then diagnose and take the recommended way out', async () => {
      const paused = await driveUntil(
        page,
        api,
        executionId,
        (current) => isPausedOnFailure(current.status) && current.failure !== null && current.failure !== undefined,
        { timeoutMs: 120_000, label: 'the injected fault to pause the run' },
      )
      expect(paused.failure?.code).toBe(FAULT_CODE)
      expect(paused.failure?.stepKey).toBe(planned.faultStep.key)

      await expect(headerBadge(page, /^(?:Paused|Diagnosing)$/)).toBeVisible({
        timeout: 15_000,
      })
      const panel = page.getByTestId(testIds.execution.failurePanel)
      await expect(panel).toBeVisible({ timeout: 15_000 })
      await expect(panel).toContainText(FAULT_CODE)

      // The remediation list: the failure's options plus "Give an instruction".
      await expect
        .poll(() => panel.getByRole('radio').count(), {
          message: 'remediation options are listed',
        })
        .toBeGreaterThanOrEqual(3)
      await resolveRecommendedInUi(page)
    })

    const finished = await test.step('Approve and resolve what remains, then wait for Succeeded', async () => {
      const done = await driveUntil(page, api, executionId, (current) => isTerminal(current.status), {
        timeoutMs: 180_000,
        label: 'the dry run to finish',
        resolve: true,
      })
      expect(done.status).toBe('succeeded')
      await expect(headerBadge(page, 'Succeeded')).toBeVisible({
        timeout: 15_000,
      })

      const log = page.getByRole('region', { name: 'Event log' })
      await expect(log.getByText('checkpoint', { exact: true }).first()).toBeVisible({ timeout: 15_000 })
      expect((await listEvents(api, executionId)).some((event) => event.type === 'checkpoint')).toBe(true)
      return done
    })

    await test.step(`The pattern lists ${project.name} under "Used by"`, async () => {
      const proposal = await getProposal(api, proposalId)
      const accepted = proposal.options.find((option) => option.id === proposal.acceptedOptionId)
      const patternId = finished.patternId ?? accepted?.patternId
      if (!patternId) throw new Error('Neither the execution nor the accepted option names a pattern')

      const names = new Set([project.name, project.repoFullName])
      const pattern = await poll(
        () => getPattern(api, patternId),
        (current) => current.usedBy.some((name) => names.has(name)),
        {
          timeoutMs: 60_000,
          intervalMs: 2_000,
          label: `pattern ${patternId} to list ${project.name} under usedBy`,
        },
      )

      await page.goto('/knowledge/patterns')
      const row = page.getByRole('row').filter({ hasText: pattern.name }).filter({ hasText: project.name })
      await expect(row.first()).toBeVisible({ timeout: 15_000 })
    })

    guard.assertNoExceptions()
  })
})
