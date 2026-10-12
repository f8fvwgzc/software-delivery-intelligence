/*
 * Session and workspace (CONTRACT.md §18): a signed-out browser only sees
 * the login page; a signed-in one lands in the shell with its workspace
 * switcher, and the three governance pages render for an admin.
 */
import type { APIRequestContext } from '@playwright/test'

import { getMe, type Me } from './helpers/auth'
import { expect, test } from './fixtures'
import { isApiReachable, newApiContext, watchConsole } from './helpers/stack'

let api: APIRequestContext
let apiUp = false
let me: Me | null = null

test.beforeAll(async ({ playwright }) => {
  api = await newApiContext(playwright, test.info().project.use.baseURL)
  apiUp = await isApiReachable(api)
  me = apiUp ? await getMe(api) : null
})

test.afterAll(async () => {
  await api?.dispose()
})

test.describe('smoke: session', () => {
  test('a signed-out browser is sent to the login page', async ({ browser, baseURL }) => {
    test.skip(!apiUp, 'needs a reachable stack')
    // A context of its own: the shared fixture signs in before the page opens.
    const context = await browser.newContext({ baseURL })
    const page = await context.newPage()
    const guard = watchConsole(page)
    try {
      await page.goto('/projects')
      await expect(page).toHaveURL(/\/login\?returnTo=%2Fprojects$/)
      await expect(page.getByRole('heading', { level: 1, name: 'Sign in with GitHub' })).toBeVisible()
      await expect(page.getByRole('button', { name: 'Continue with GitHub' })).toBeEnabled()
      await expect(page.getByRole('link', { name: /install the GitHub App/ })).toBeVisible()
      await expect(page.getByRole('link', { name: 'What we store' })).toHaveAttribute('href', /\/docs\/trust$/)
      // No shell around it.
      await expect(page.locator('a[data-sidebar="menu-button"]')).toHaveCount(0)
      guard.assertClean({ apiUp: true })
    } finally {
      await context.close()
    }
  })

  test('continue with GitHub signs in through the dev login and returns to the page', async ({ browser, baseURL }) => {
    test.skip(!apiUp, 'needs a reachable stack with SDI_DEV_LOGIN=1')
    const context = await browser.newContext({ baseURL })
    const page = await context.newPage()
    try {
      await page.goto('/login?returnTo=%2Fprojects')
      await page.getByRole('button', { name: 'Continue with GitHub' }).click()
      await expect(page).toHaveURL(/\/projects$/, { timeout: 20_000 })
      await expect(page.getByRole('heading', { level: 1, name: 'Projects', exact: true })).toBeVisible()
    } finally {
      await context.close()
    }
  })

  test('the trust documents are readable without a session', async ({ browser, baseURL }) => {
    const context = await browser.newContext({ baseURL })
    const page = await context.newPage()
    const guard = watchConsole(page)
    try {
      await page.goto('/docs/trust')
      await expect(page.getByRole('navigation', { name: 'Trust documents' })).toBeVisible()
      await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 20_000 })
      await page.getByRole('navigation', { name: 'Trust documents' }).getByRole('link', { name: 'Privacy', exact: true }).click()
      await expect(page).toHaveURL(/\/docs\/privacy$/)
      await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 20_000 })
      guard.assertClean({ apiUp })
    } finally {
      await context.close()
    }
  })

  test('the login page sends a signed-in user back in', async ({ page }) => {
    test.skip(!me, 'needs a reachable stack and a session')
    await page.goto('/login')
    await expect(page).toHaveURL(/\/$/)
    await expect(page.getByRole('heading', { level: 1, name: 'Overview', exact: true })).toBeVisible()
  })

  test('the overview shows the workspace switcher and the user menu', async ({ page }) => {
    test.skip(!me, 'needs a reachable stack and a session')
    const guard = watchConsole(page)
    const workspace = me!.workspaces.find((candidate) => candidate.id === me!.currentWorkspaceId) ?? me!.workspaces[0]

    await page.goto('/')
    await expect(page.getByRole('heading', { level: 1, name: 'Overview', exact: true })).toBeVisible()

    const switcher = page.getByRole('button', { name: new RegExp(`^Workspace: ${workspace.name}\\.`) })
    await expect(switcher).toBeVisible()
    await switcher.click()
    const menu = page.getByRole('menu')
    await expect(menu.getByRole('menuitem', { name: new RegExp(workspace.name) })).toBeVisible()
    await expect(menu.getByRole('menuitem', { name: 'Install on an organisation' })).toBeVisible()
    await page.keyboard.press('Escape')

    await expect(page.getByRole('button', { name: new RegExp(`^Signed in as ${me!.user.login}\\.`) })).toBeVisible()
    guard.assertClean({ apiUp: true })
  })

  test('/workspace/members lists the signed-in user with a role', async ({ page }) => {
    test.skip(!me, 'needs a reachable stack and a session')
    const guard = watchConsole(page)
    await page.goto('/workspace/members')
    await expect(page.getByRole('heading', { level: 1, name: 'Members', exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Sync from GitHub' })).toBeVisible()
    const row = page.getByRole('row').filter({ hasText: me!.user.login })
    await expect(row).toBeVisible()
    await expect(row.getByRole('combobox', { name: `Role override for ${me!.user.login}` })).toBeVisible()
    guard.assertClean({ apiUp: true })
  })

  test('/workspace/audit renders entries or its empty state, with the prefix filter', async ({ page }) => {
    test.skip(!me, 'needs a reachable stack and a session')
    const guard = watchConsole(page)
    await page.goto('/workspace/audit')
    await expect(page.getByRole('heading', { level: 1, name: 'Audit', exact: true })).toBeVisible()
    const table = page.getByRole('table')
    const empty = page.getByText('Nothing audited yet')
    await expect(table.or(empty).first()).toBeVisible()
    await page.getByRole('radio', { name: 'Data' }).click()
    await expect(page.getByRole('textbox', { name: 'Action prefix' })).toHaveValue('data.')
    guard.assertClean({ apiUp: true })
  })

  test('/workspace/data shows the storage table, the controls and the actions', async ({ page }) => {
    test.skip(!me, 'needs a reachable stack and a session')
    const guard = watchConsole(page)
    await page.goto('/workspace/data')
    await expect(page.getByRole('heading', { level: 1, name: 'Data & privacy', exact: true })).toBeVisible()
    await expect(page.getByRole('cell', { name: 'Audit entries' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Save controls' })).toBeDisabled()
    await expect(page.getByRole('radio', { name: 'Never stored' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Export' })).toBeEnabled()
    await expect(page.getByRole('button', { name: 'Purge code cache' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Delete workspace' })).toBeVisible()
    guard.assertClean({ apiUp: true })
  })

  test('sign out from the user menu lands on the login page', async ({ page }) => {
    test.skip(!me, 'needs a reachable stack and a session')
    await page.goto('/')
    await page.getByRole('button', { name: new RegExp(`^Signed in as ${me!.user.login}\\.`) }).click()
    await page.getByRole('menuitem', { name: 'Sign out' }).click()
    await expect(page).toHaveURL(/\/login$/, { timeout: 20_000 })
    await expect(page.getByRole('button', { name: 'Continue with GitHub' })).toBeVisible()
  })
})
