/*
 * Policies page: an editable decision changes and survives a reload; a
 * default that must not be relaxed shows its locked state and the gateway
 * refuses to change it (CONTRACT.md §5.2).
 */
import { expect, test, type APIRequestContext } from '@playwright/test'

import { testIds } from '../src/test-ids'
import { ApiError, listPolicies, updatePolicy, type Policy, type PolicyDecision } from './helpers/api'
import { isApiReachable, newApiContext, watchConsole } from './helpers/stack'

/** Display labels, as `features/policies/lib/labels.ts` renders them. */
const LABEL: Record<PolicyDecision, string> = {
  unspecified: 'Unset',
  auto: 'Auto',
  approval: 'Approval',
  required: 'Required',
  blocked: 'Blocked',
}

let api: APIRequestContext
let apiUp = false
let editable: Policy | undefined
let locked: Policy | undefined
let original: PolicyDecision | undefined

test.beforeAll(async ({ playwright }) => {
  api = await newApiContext(playwright, test.info().project.use.baseURL)
  apiUp = await isApiReachable(api)
  if (!apiUp) return

  const policies = await listPolicies(api)
  editable =
    policies.find((policy) => policy.actionType === 'tests.run_local' && policy.editable) ??
    policies.find((policy) => policy.editable && policy.decision !== 'required')
  locked =
    policies.find((policy) => policy.actionType === 'deploy.production' && !policy.editable) ??
    policies.find((policy) => !policy.editable)
  original = editable?.decision
})

test.afterAll(async () => {
  // Leave the stack as it was found.
  if (api && editable && original) {
    await updatePolicy(api, editable.actionType, original).catch(() => undefined)
  }
  await api?.dispose()
})

test('an editable decision persists across a reload', async ({ page }) => {
  test.skip(!apiUp, 'needs a reachable stack')
  test.skip(!editable, 'no editable policy is seeded')
  const policy = editable as Policy
  const target: PolicyDecision = policy.decision === 'auto' ? 'approval' : 'auto'

  const guard = watchConsole(page)
  await page.goto('/policies')

  const select = page.getByRole('combobox', { name: `Decision for ${policy.label}` })
  await expect(select).toContainText(LABEL[policy.decision])

  await select.click()
  await page.getByRole('option', { name: new RegExp(`^${LABEL[target]}`) }).click()

  // The toast confirms, the control shows the new value, the gateway agrees.
  await expect(page.getByText(`${policy.label}: ${LABEL[target].toLowerCase()}`)).toBeVisible()
  await expect(select).toContainText(LABEL[target])
  await expect
    .poll(async () => (await listPolicies(api)).find((row) => row.actionType === policy.actionType)?.decision)
    .toBe(target)

  await page.reload()
  await expect(page.getByRole('combobox', { name: `Decision for ${policy.label}` })).toContainText(LABEL[target])

  guard.assertClean({ apiUp: true })
})

test('a default that cannot be relaxed shows a locked decision', async ({ page }) => {
  test.skip(!apiUp, 'needs a reachable stack')
  test.skip(!locked, 'no non-editable policy is seeded')
  const policy = locked as Policy

  const guard = watchConsole(page)
  await page.goto('/policies')

  const row = page.getByRole('row').filter({ hasText: policy.label }).first()
  const cell = row.getByTestId(testIds.policies.lockedDecision)
  await expect(cell).toBeVisible()
  await expect(cell).toContainText(LABEL[policy.decision])
  await expect(row.getByRole('combobox')).toHaveCount(0)

  // Keyboard reachable: focusing the locked cell explains why.
  await cell.focus()
  await expect(page.getByText('Cannot be relaxed').first()).toBeVisible()

  // The gateway refuses the relaxation with 412 (FAILED_PRECONDITION).
  const refusal = await updatePolicy(api, policy.actionType, 'auto').then(
    () => null,
    (error: unknown) => error,
  )
  expect(refusal).toBeInstanceOf(ApiError)
  expect((refusal as ApiError).status).toBe(412)

  guard.assertClean({ apiUp: true })
})
