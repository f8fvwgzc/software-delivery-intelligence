/*
 * The GitOps engine (CONTRACT.md §24): a project's Applications tab, the
 * application page (resources with their diff, manifests, history with
 * rollback, Sync / Refresh / Edit / Delete) and Workspace › Clusters.
 *
 * Most cases fulfil the §24 routes in the browser with answers typed by the
 * gateway's schema, so they hold whatever runs behind the gateway and can
 * never sync, roll back or delete anything: every write in this file ends at
 * `page.route`, and one a case did not fulfil fails it. They run against
 * the dev stack when it answers; without one, against the built app
 * (`pnpm build`), which `helpers/offline.ts` serves to the page itself
 * together with the session and the shell's own requests. One case per page
 * leaves the routes alone (stack only): whatever the gateway answers, every
 * region shows content or its own error with a retry, never a blank page.
 *
 * The last block needs no browser: what an `application` event invalidates.
 */
import { AxeBuilder } from '@axe-core/playwright'
import type { APIRequestContext, Locator, Page, Request, Route } from '@playwright/test'

import type { components } from '../src/lib/api/schema'
import { FAMILIES_BY_ENTITY, gatedFamilies } from '../src/lib/live/event-families'
import { expect, test } from './fixtures'
import { listProjects, type Project as StackProject } from './helpers/api'
import type { Me } from './helpers/auth'
import { hasBuiltApp, OFFLINE_WORKSPACE_ID, serveBuiltApp } from './helpers/offline'
import { headerBadge, isApiReachable, newApiContext, waitForSettled, watchConsole } from './helpers/stack'

type Schemas = components['schemas']
type Application = Schemas['Application']
type ResourceStatus = Schemas['ResourceStatus']
type ApplicationDiff = Schemas['ApplicationDiff']
type HistoryEntry = Schemas['HistoryEntry']
type Cluster = Schemas['Cluster']
type Project = Schemas['Project']
type Person = Schemas['Person']
type ErrorBody = Schemas['ErrorBody']
type Role = Schemas['Role']

let api: APIRequestContext
/** `stack`: the dev stack answers; `offline`: only the built app is there; `none`: neither. */
let mode: 'stack' | 'offline' | 'none' = 'none'
let stackProject: StackProject | null = null

test.beforeAll(async ({ playwright }) => {
  api = await newApiContext(playwright, test.info().project.use.baseURL)
  const apiUp = await isApiReachable(api)
  mode = apiUp ? 'stack' : hasBuiltApp() ? 'offline' : 'none'
  stackProject = apiUp ? ((await listProjects(api))[0] ?? null) : null
})

test.afterAll(async () => {
  await api?.dispose()
})

/* Answers */

const json = (body: unknown, status = 200) => ({ status, contentType: 'application/json', body: JSON.stringify(body) })
const refusal = (message: string, status = 412) => json({ code: 'failed_precondition', message } satisfies ErrorBody, status)

/** Ids that exist nowhere: every page here is fed by `page.route`. */
const PROJECT_ID = 'e2e-gitops-project'
const APP_ID = 'e2e-gitops-web'
const CLUSTER_ID = 'e2e-cluster-production'
const REPO_URL = 'https://github.com/acme/shop'
const SHA = '4f9c2ab41d07e5c8b6a1f0e4d2c3b5a697081f2e'
const SYNCED_SHA = '1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d'
const FAILED_SHA = '9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a291807'
const OLDEST_SHA = '0f1e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6'
const USER_ID = '6c1f3a52-7d4e-4b8a-9f10-2e3d4c5b6a71'
const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString()

const PROJECT: Project = {
  id: PROJECT_ID,
  name: 'shop',
  repoFullName: 'acme/shop',
  repoUrl: REPO_URL,
  defaultBranch: 'main',
  description: 'The shop',
  isPrivate: true,
  hasConfig: false,
  lastScanId: null,
  lastScannedAt: null,
  maturityScore: 0,
  mode: 'observe',
  openQuestions: 0,
  primaryLanguage: 'TypeScript',
  stack: [],
  status: 'ready',
  tldr: '',
  createdAt: ago(60 * 24),
}

const OCTOCAT: Person = { id: USER_ID, login: 'octocat', name: 'The Octocat', avatarUrl: '', member: true }

function resource(overrides: Partial<ResourceStatus> & Pick<ResourceStatus, 'kind' | 'name'>): ResourceStatus {
  return {
    group: 'apps',
    version: 'v1',
    namespace: 'shop',
    syncStatus: 'synced',
    healthStatus: 'healthy',
    healthMessage: null,
    hook: false,
    syncWave: 0,
    ...overrides,
  }
}

const RESOURCES: ResourceStatus[] = [
  resource({ kind: 'Deployment', name: 'web', syncStatus: 'out_of_sync', syncWave: 1 }),
  resource({
    kind: 'Deployment',
    name: 'worker',
    syncWave: 1,
    healthStatus: 'progressing',
    healthMessage: 'Waiting for rollout to finish: 1 of 2 updated replicas are available',
  }),
  resource({ kind: 'Service', name: 'web', group: '' }),
  resource({ kind: 'ConfigMap', name: 'web-config', group: '' }),
  resource({ kind: 'Job', name: 'migrate', group: 'batch', hook: true, syncWave: -1 }),
  resource({ kind: 'Secret', name: 'web-env', group: '', syncStatus: 'missing', healthStatus: 'missing' }),
]

function application(overrides: Partial<Application> = {}): Application {
  return {
    id: APP_ID,
    projectId: PROJECT_ID,
    name: 'web',
    clusterId: CLUSTER_ID,
    clusterName: 'production',
    namespace: 'shop',
    source: {
      repo: null,
      revision: 'main',
      path: 'deploy/web',
      type: 'kustomize',
      recurse: false,
      helmReleaseName: null,
      helmValueFiles: [],
      helmParameters: {},
      kustomizeImages: ['ghcr.io/acme/web=ghcr.io/acme/web:1.4.2'],
    },
    syncPolicy: { automated: false, selfHeal: false, prune: false, createNamespace: true, retryLimit: 0 },
    syncStatus: 'out_of_sync',
    healthStatus: 'progressing',
    healthMessage: null,
    conditions: [],
    operation: null,
    outOfSyncCount: 2,
    resourceCount: RESOURCES.length,
    resources: RESOURCES,
    revision: SHA,
    syncedRevision: SYNCED_SHA,
    reconciledAt: ago(2),
    createdAt: ago(60 * 24 * 3),
    updatedAt: ago(2),
    ...overrides,
  }
}

const WEB_DIFF = [
  '--- live',
  '+++ desired',
  '@@ -18,7 +18,7 @@',
  '       containers:',
  '         - name: web',
  '-          image: ghcr.io/acme/web:1.4.1',
  '+          image: ghcr.io/acme/web:1.4.2',
  '           ports:',
  '             - containerPort: 8080',
].join('\n')

const DIFF: ApplicationDiff = {
  revision: SHA,
  resources: RESOURCES.map((row) => ({
    group: row.group,
    kind: row.kind,
    namespace: row.namespace,
    name: row.name,
    syncStatus: row.syncStatus,
    diff: row.kind === 'Deployment' && row.name === 'web' ? WEB_DIFF : null,
    live: row.syncStatus === 'missing' ? null : `kind: ${row.kind}`,
    target: `kind: ${row.kind}`,
  })),
}

function entry(overrides: Partial<HistoryEntry> & Pick<HistoryEntry, 'id' | 'revision'>): HistoryEntry {
  return {
    applicationId: APP_ID,
    automated: false,
    executionId: null,
    initiatedBy: USER_ID,
    message: null,
    phase: 'succeeded',
    source: application().source,
    startedAt: ago(62),
    finishedAt: ago(60),
    ...overrides,
  }
}

const HISTORY: HistoryEntry[] = [
  entry({ id: 'h-current', revision: SYNCED_SHA, executionId: 'e2e-exec-current', startedAt: ago(32), finishedAt: ago(30) }),
  entry({
    id: 'h-failed',
    revision: FAILED_SHA,
    phase: 'failed',
    automated: true,
    initiatedBy: 'system',
    message: 'Deployment shop/web: ImagePullBackOff',
    startedAt: ago(122),
    finishedAt: ago(120),
  }),
  entry({ id: 'h-oldest', revision: OLDEST_SHA, executionId: 'e2e-exec-oldest', startedAt: ago(60 * 26), finishedAt: ago(60 * 26 - 3) }),
]

function cluster(overrides: Partial<Cluster> & Pick<Cluster, 'id' | 'name'>): Cluster {
  return {
    status: 'connected',
    message: null,
    server: `https://k8s.${overrides.name}.acme.example:6443`,
    serverVersion: 'v1.31.2',
    applicationCount: 0,
    checkedAt: ago(5),
    kubeconfigSecret: `KUBECONFIG_${overrides.name.toUpperCase()}`,
    context: null,
    defaultNamespace: null,
    namespaces: [],
    createdAt: ago(60 * 24 * 10),
    updatedAt: ago(5),
    ...overrides,
  }
}

const CLUSTERS: Cluster[] = [
  cluster({ id: CLUSTER_ID, name: 'production', applicationCount: 2, defaultNamespace: 'shop', namespaces: ['shop', 'payments'] }),
  cluster({
    id: 'e2e-cluster-staging',
    name: 'staging',
    status: 'unreachable',
    message: 'dial tcp 10.0.4.12:6443: i/o timeout',
    server: 'https://10.0.4.12:6443',
    serverVersion: null,
  }),
  cluster({ id: 'e2e-cluster-edge', name: 'edge', status: 'unknown', server: null, serverVersion: null, checkedAt: null }),
]

/** What a kubeconfig looks like: it must never be on screen. */
const KUBECONFIG = [
  'apiVersion: v1',
  'kind: Config',
  'clusters:',
  '  - name: lab',
  '    cluster:',
  '      server: https://k8s.lab.acme.example:6443',
  '      certificate-authority-data: LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0t',
  'users:',
  '  - name: deployer',
  '    user:',
  '      token: e2e-not-a-real-token',
  'contexts:',
  '  - name: lab',
  '    context: { cluster: lab, user: deployer }',
  'current-context: lab',
].join('\n')

/* Routes */

const pathIs = (pathname: string | RegExp) => (url: URL) => (typeof pathname === 'string' ? url.pathname === pathname : pathname.test(url.pathname))

/** Fulfils one method of one path; other methods fall through to the routes registered before. */
async function on(page: Page, method: string, pathname: string | RegExp, handler: (route: Route, request: Request) => unknown) {
  await page.route(pathIs(pathname), (route) => (route.request().method() === method ? handler(route, route.request()) : route.fallback()))
}

/** The session as a member of `role` sees it: the same answer, with the role replaced. */
async function actAs(page: Page, role: Role) {
  await page.route(pathIs('/api/v1/auth/me'), async (route) => {
    const response = await route.fetch()
    const me = (await response.json()) as Me
    await route.fulfill({ response, json: { ...me, workspaces: me.workspaces.map((workspace) => ({ ...workspace, myRole: role })) } })
  })
}

/** Nothing that writes may reach a gateway from this file: a write the case did not fulfil fails it. */
async function forbidWrites(page: Page): Promise<string[]> {
  const leaked: string[] = []
  await page.route(/\/api\/v1\/(applications|clusters|workspaces\/[^/]+\/secrets)(\/|\?|$)/, (route) => {
    if (route.request().method() === 'GET') return route.fallback()
    leaked.push(`${route.request().method()} ${new URL(route.request().url()).pathname}`)
    return route.fulfill(json({ code: 'internal', message: 'not fulfilled by the test' } satisfies ErrorBody, 500))
  })
  return leaked
}

/**
 * Opens a case at `role`: the built app and the shell's answers offline, the
 * real session (role replaced) on the stack; then the guard against writes,
 * the people lookup and the project behind the applications.
 */
async function prepare(page: Page, role: Role): Promise<string[]> {
  if (mode === 'offline') await serveBuiltApp(page, { role })
  else await actAs(page, role)
  const leaked = await forbidWrites(page)
  await on(page, 'GET', '/api/v1/people', (route, request) => {
    const ids = new URL(request.url()).searchParams.get('ids')?.split(',') ?? []
    return route.fulfill(json(ids.includes(USER_ID) ? [OCTOCAT] : []))
  })
  await on(page, 'GET', `/api/v1/projects/${PROJECT_ID}`, (route) => route.fulfill(json(PROJECT)))
  return leaked
}

/** The application page's reads, each answered from the case's current value. */
async function mockApplicationPage(page: Page, initial: { application?: Application; history?: HistoryEntry[] } = {}) {
  const mock = {
    application: initial.application ?? application(),
    history: initial.history ?? HISTORY,
    diffRequests: 0,
    manifestRequests: 0,
    historyRequests: 0,
  }
  await on(page, 'GET', `/api/v1/applications/${APP_ID}`, (route) => route.fulfill(json(mock.application)))
  await on(page, 'GET', `/api/v1/applications/${APP_ID}/history`, (route) => {
    mock.historyRequests += 1
    return route.fulfill(json(mock.history))
  })
  await on(page, 'GET', `/api/v1/applications/${APP_ID}/diff`, (route) => {
    mock.diffRequests += 1
    return route.fulfill(json(DIFF))
  })
  return mock
}

const region = (page: Page, name: string) => page.getByRole('region', { name, exact: true })

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
async function expectRegionSettled(target: Locator, content: Locator) {
  await expect(target).toBeVisible({ timeout: 20_000 })
  const failed = target.getByRole('alert').filter({ has: target.page().getByRole('button', { name: 'Try again' }) })
  await expect(content.or(failed).first()).toBeVisible({ timeout: 30_000 })
}

/** A control that is off says why: its wrapper takes the focus the disabled button cannot, and shows the reason. */
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

/** Nothing on screen names a user by id, and nothing looks like a kubeconfig. */
async function expectNothingPrivate(page: Page) {
  const text = await page.locator('body').innerText()
  expect(text, 'a raw user id on screen').not.toContain(USER_ID)
  for (const marker of ['certificate-authority-data', 'client-key-data', 'current-context', 'e2e-not-a-real-token']) {
    expect(text, `kubeconfig content (${marker}) on screen`).not.toContain(marker)
  }
}

const requireApp = () => test.skip(mode === 'none', 'needs the dev stack, or a built app (pnpm build) to serve offline')

/* The Applications tab */

test.describe('applications tab', () => {
  test.beforeEach(requireApp)

  test('lists each application with its sync, health, destination, revision and automation', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'viewer')
    const requested = recordApiPaths(page)
    const running = application({
      id: 'e2e-gitops-api',
      name: 'api',
      resources: [],
      operation: { kind: 'sync', phase: 'running', initiatedBy: USER_ID, executionId: 'e2e-exec-api', startedAt: ago(1), revision: SHA },
    })
    const automated = application({
      id: 'e2e-gitops-docs',
      name: 'docs',
      resources: [],
      syncStatus: 'synced',
      healthStatus: 'healthy',
      outOfSyncCount: 0,
      namespace: null,
      source: { ...application().source, type: null, path: null, kustomizeImages: [] },
      syncPolicy: { automated: true, selfHeal: true, prune: true, createNamespace: false, retryLimit: 0 },
    })
    const broken = application({
      id: 'e2e-gitops-web',
      resources: [],
      healthStatus: 'degraded',
      conditions: ['The source could not be rendered: deploy/web has no kustomization.yaml'],
    })
    await on(page, 'GET', '/api/v1/applications', (route, request) => {
      expect(new URL(request.url()).searchParams.get('projectId')).toBe(PROJECT_ID)
      return route.fulfill(json([running, automated, broken]))
    })

    await page.goto(`/projects/${PROJECT_ID}/applications`)

    await expect(page.getByRole('heading', { level: 1, name: 'Applications', exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(page.getByRole('navigation', { name: 'Project sections' }).getByRole('link', { name: 'Applications' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    const table = region(page, 'Applications').getByRole('table', { name: 'Applications' })
    await expect(table.getByRole('row')).toHaveCount(4)

    // Sorted by name; each name opens its page.
    const rows = table.getByRole('row')
    await expect(rows.nth(1).getByRole('link', { name: 'api' })).toHaveAttribute('href', '/applications/e2e-gitops-api')
    await expect(rows.nth(2).getByRole('link', { name: 'docs' })).toBeVisible()
    await expect(rows.nth(3).getByRole('link', { name: 'web' })).toBeVisible()

    // The operation in flight replaces the sync tag; the health keeps pulsing.
    await expect(rows.nth(1).getByText('Sync running', { exact: true })).toBeVisible()
    await expect(rows.nth(1).getByText('Progressing', { exact: true })).toBeVisible()
    await expect(rows.nth(2).getByText('Synced', { exact: true })).toBeVisible()
    await expect(rows.nth(2).getByText('Healthy', { exact: true })).toBeVisible()
    await expect(rows.nth(3).getByText('Out of sync', { exact: true })).toBeVisible()
    await expect(rows.nth(3).getByText('2 of 6 differ')).toBeVisible()
    await expect(rows.nth(3).getByText('Degraded', { exact: true })).toBeVisible()
    // The first condition, as a warning line under the name.
    await expect(rows.nth(3).getByText('deploy/web has no kustomization.yaml')).toBeVisible()

    // Where it deploys, and what renders it.
    await expect(rows.nth(3).getByText('production', { exact: true })).toBeVisible()
    await expect(rows.nth(3).getByText('shop', { exact: true })).toBeVisible()
    await expect(rows.nth(2).getByText('cluster default')).toBeVisible()
    await expect(rows.nth(3).getByText('deploy/web', { exact: true })).toBeVisible()
    await expect(rows.nth(3).getByText('Kustomize')).toBeVisible()
    await expect(rows.nth(2).getByText('repository root')).toBeVisible()
    await expect(rows.nth(2).getByText('Detected')).toBeVisible()

    // The compared commit links to GitHub, short.
    await expect(rows.nth(3).getByRole('link', { name: 'Commit 4f9c2ab on GitHub' })).toHaveAttribute('href', `${REPO_URL}/commit/${SHA}`)

    // What syncs on its own is read off the row.
    await expect(rows.nth(2).getByText('Auto sync', { exact: true })).toBeVisible()
    await expect(rows.nth(2).getByText('Self heal', { exact: true })).toBeVisible()
    await expect(rows.nth(2).getByText('Prune', { exact: true })).toBeVisible()
    await expect(rows.nth(3).getByText('Manual', { exact: true })).toBeVisible()

    // A viewer is not offered to declare one.
    await expect(page.getByRole('button', { name: 'New application' })).toHaveCount(0)

    // One endpoint of its own on first paint; the clusters wait for the editor.
    expect(requested.filter((path) => path === '/api/v1/applications')).not.toHaveLength(0)
    expect(requested.filter((path) => path.startsWith('/api/v1/clusters') || path.includes('/diff') || path.includes('/manifests'))).toEqual([])

    await expectAccessible(page)
    await expectNothingPrivate(page)
    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('an empty project explains applications; an admin declares one and reads a refusal under its field', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'admin')
    await on(page, 'GET', '/api/v1/applications', (route) => route.fulfill(json([])))
    await on(page, 'GET', '/api/v1/clusters', (route) => route.fulfill(json(CLUSTERS)))
    const bodies: Schemas['UpsertApplicationRequest'][] = []
    await on(page, 'POST', '/api/v1/applications', (route, request) => {
      const body = request.postDataJSON() as Schemas['UpsertApplicationRequest']
      bodies.push(body)
      if (bodies.length === 1) return route.fulfill(refusal('name: an application called web exists in this project', 409))
      return route.fulfill(json(application({ id: 'e2e-gitops-new', name: body.name, resources: [] }), 201))
    })

    await page.goto(`/projects/${PROJECT_ID}/applications`)

    const panel = region(page, 'Applications')
    await expect(panel.getByText('No applications yet')).toBeVisible({ timeout: 20_000 })
    await expect(panel.getByText(/kept in step with one namespace of a cluster/)).toBeVisible()

    await panel.getByRole('button', { name: 'New application' }).click()
    const dialog = page.getByRole('dialog', { name: 'New application' })
    await expect(dialog).toBeVisible()
    await expect(dialog.getByText(/Saving applies nothing/)).toBeVisible()
    await expect(dialog.getByRole('link', { name: 'Register a cluster' })).toHaveAttribute('href', '/workspace/clusters')

    // Nothing is sent before the required fields are there; focus lands on the first one that is missing.
    await dialog.getByRole('button', { name: 'Create application' }).click()
    await expect(dialog.getByText('Give the application a name.')).toBeVisible()
    await expect(dialog.getByLabel('Name', { exact: true })).toBeFocused()
    expect(bodies).toHaveLength(0)

    await dialog.getByLabel('Name', { exact: true }).fill('web')
    await dialog.getByRole('combobox', { name: 'Cluster' }).click()
    await page.getByRole('option', { name: /^production/ }).click()
    await expect(dialog.getByRole('combobox', { name: 'Cluster' })).toContainText('production')
    // The namespace's hint follows the cluster: this one is watched in two namespaces only.
    await expect(dialog.getByText('This cluster is watched only in shop, payments.')).toBeVisible()
    await dialog.getByRole('textbox', { name: /^Namespace/ }).fill('shop')
    await dialog.getByLabel('Path').fill('/charts/web/')

    // Detect shows every type's options; Helm shows its own only.
    await expect(dialog.getByLabel('Kustomize: image overrides')).toBeVisible()
    await dialog.getByRole('combobox', { name: 'Type' }).click()
    await page.getByRole('option', { name: /^Helm/ }).click()
    await expect(dialog.getByLabel(/image overrides/)).toHaveCount(0)
    await dialog.getByLabel('Parameters').fill('image.tag=1.4.2\nreplicaCount=3')
    await dialog.getByLabel('Values files').fill('values.yaml\nvalues-production.yaml')

    // Self heal and prune refine automated sync: off until it is on.
    const selfHeal = dialog.getByRole('switch', { name: 'Self heal' })
    await expect(selfHeal).toBeDisabled()
    await expect(dialog.getByText(/the only way anything reaches the cluster without an approval/)).toBeVisible()
    await dialog.getByRole('switch', { name: 'Automated sync' }).click()
    await expect(selfHeal).toBeEnabled()
    await selfHeal.click()

    // The server's refusal sits under the field it names, and focus goes there.
    await dialog.getByRole('button', { name: 'Create application' }).click()
    const name = dialog.getByLabel('Name', { exact: true })
    await expect(dialog.getByText('An application called web exists in this project')).toBeVisible()
    await expect(name).toHaveAttribute('aria-invalid', 'true')
    await expect(name).toBeFocused()
    // An edit answers the refusal.
    await name.fill('web-eu')
    await expect(dialog.getByText('An application called web exists in this project')).toHaveCount(0)

    await dialog.getByRole('button', { name: 'Create application' }).click()
    await expect(page.getByText('web-eu declared')).toBeVisible()
    await expect(dialog).toBeHidden()

    expect(bodies).toHaveLength(2)
    expect(bodies[1]).toEqual({
      projectId: PROJECT_ID,
      name: 'web-eu',
      clusterId: CLUSTER_ID,
      namespace: 'shop',
      source: {
        repo: null,
        revision: null,
        path: 'charts/web',
        type: 'helm',
        recurse: false,
        helmReleaseName: null,
        helmValueFiles: ['values.yaml', 'values-production.yaml'],
        helmParameters: { 'image.tag': '1.4.2', replicaCount: '3' },
        kustomizeImages: [],
      },
      syncPolicy: { automated: true, selfHeal: true, prune: false, createNamespace: false, retryLimit: null },
    } satisfies Schemas['UpsertApplicationRequest'])

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })
})

/* The application page */

test.describe('application page', () => {
  test.beforeEach(requireApp)

  test('sync and health, conditions, the operation in flight, and resources by kind with their diff on demand', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'viewer')
    const requested = recordApiPaths(page)
    const mock = await mockApplicationPage(page, {
      application: application({
        conditions: ['The cluster production did not answer: dial tcp 10.0.1.5:6443: i/o timeout'],
        operation: { kind: 'sync', phase: 'running', initiatedBy: USER_ID, executionId: 'e2e-exec-running', startedAt: ago(1), revision: SHA },
      }),
    })

    await page.goto(`/applications/${APP_ID}`)

    await expect(page.getByRole('heading', { level: 1, name: 'web', exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(headerBadge(page, 'Out of sync')).toBeVisible()
    await expect(headerBadge(page, 'Progressing')).toBeVisible()
    await expect(headerBadge(page, 'Manual')).toBeVisible()
    await expect(page.getByRole('link', { name: 'shop applications' })).toHaveAttribute('href', `/projects/${PROJECT_ID}/applications`)
    // The crumb is the application's name.
    await expect(page.getByRole('navigation', { name: 'breadcrumb' }).getByText('web', { exact: true })).toBeVisible()

    await expect(region(page, 'Conditions').getByText(/did not answer: dial tcp/)).toBeVisible()

    // The operation in flight, who started it (by login, never an id), and its execution.
    const notice = page.getByRole('status').filter({ hasText: 'Sync running' })
    await expect(notice).toBeVisible()
    await expect(notice.getByText('octocat')).toBeVisible()
    await expect(notice.getByRole('link', { name: 'Open the execution' })).toHaveAttribute('href', '/executions/e2e-exec-running')

    // Kinds in the order a sync applies them.
    const resources = region(page, 'Resources')
    await expect(resources.getByText('6 resources')).toBeVisible()
    await expect(resources.getByRole('heading', { level: 3 })).toHaveText([/^Secret/, /^ConfigMap/, /^Service/, /^Deployment/, /^Job/])
    await expect(resources.getByRole('heading', { level: 3, name: /^Deployment/ })).toContainText('1 out of sync')
    const job = resources.getByRole('list', { name: 'Job resources' })
    await expect(job.getByText('Hook', { exact: true })).toBeVisible()
    await expect(job.getByText('wave -1')).toBeVisible()
    await expect(resources.getByRole('list', { name: 'Secret resources' }).getByText('Missing').first()).toBeVisible()
    await expect(resources.getByText('Waiting for rollout to finish: 1 of 2 updated replicas are available').first()).toBeVisible()

    // The diff is read only when a resource's diff is opened, and once for all of them.
    expect(mock.diffRequests).toBe(0)
    await resources.getByRole('button', { name: 'Show the diff of Deployment shop/web' }).click()
    const diff = page.getByLabel('Diff of Deployment shop/web')
    await expect(diff.getByText('image: ghcr.io/acme/web:1.4.2')).toBeVisible()
    await expect(diff.getByText('image: ghcr.io/acme/web:1.4.1')).toBeVisible()
    await expect(resources.getByText('+1', { exact: true })).toBeVisible()
    await resources.getByRole('button', { name: 'Show the diff of Service shop/web' }).click()
    await expect(resources.getByText('No differences: the cluster matches the repository.')).toBeVisible()
    await resources.getByRole('button', { name: 'Show the diff of Secret shop/web-env' }).click()
    await expect(resources.getByText('Not in the cluster yet: a sync creates it.')).toBeVisible()
    expect(mock.diffRequests).toBe(1)
    // A folded kind hides its rows.
    await resources.getByRole('button', { name: /^Job/ }).click()
    await expect(resources.getByRole('list', { name: 'Job resources' })).toHaveCount(0)

    // First paint: the application and its history; never the manifests.
    expect(requested).toContain(`/api/v1/applications/${APP_ID}`)
    expect(requested).toContain(`/api/v1/applications/${APP_ID}/history`)
    expect(requested.filter((path) => path.endsWith('/manifests'))).toEqual([])

    await expectAccessible(page)
    await expectNothingPrivate(page)
    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('a viewer reads why each control is off; a member refreshes, softly or hard', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'viewer')
    await mockApplicationPage(page)

    await page.goto(`/applications/${APP_ID}`)
    await expect(page.getByRole('heading', { level: 1, name: 'web', exact: true })).toBeVisible({ timeout: 20_000 })

    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Refresh', exact: true }), 'Refreshing an application needs the member role.')
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Hard refresh' }), 'Refreshing an application needs the member role.')
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Sync', exact: true }), 'Syncing needs the approver role.')
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Edit', exact: true }), 'Changing an application needs the admin role.')
    await expectDisabledWithReason(page, page.getByRole('button', { name: 'Delete', exact: true }), 'Deleting an application needs the admin role.')
    await expectDisabledWithReason(
      page,
      region(page, 'History').getByRole('button', { name: `Roll back to ${OLDEST_SHA.slice(0, 7)}` }),
      'Rolling back needs the approver role.',
    )
    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('a member refreshes: the loader is in the pressed button, the answer lands, a hard refresh says so', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'member')
    const mock = await mockApplicationPage(page)
    const bodies: Schemas['RefreshApplicationRequest'][] = []
    let release: () => void = () => undefined
    const held = new Promise<void>((resolve) => (release = resolve))
    await on(page, 'POST', `/api/v1/applications/${APP_ID}/refresh`, async (route, request) => {
      bodies.push(request.postDataJSON() as Schemas['RefreshApplicationRequest'])
      if (bodies.length === 1) await held
      mock.application = application({ syncStatus: 'synced', outOfSyncCount: 0, healthStatus: 'healthy' })
      return route.fulfill(json(mock.application))
    })

    await page.goto(`/applications/${APP_ID}`)
    await expect(page.getByRole('heading', { level: 1, name: 'web', exact: true })).toBeVisible({ timeout: 20_000 })
    // A member may refresh but not sync.
    await expect(page.getByRole('button', { name: 'Sync', exact: true })).toBeDisabled()

    // While it works the spinner names itself ("Loading Refresh").
    const refresh = page.getByRole('button', { name: /^(Loading )?Refresh$/ })
    await refresh.click()
    await expect(refresh).toHaveAttribute('aria-busy', 'true')
    await expect(refresh.locator('[data-slot="spinner"]')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Hard refresh' })).toBeDisabled()
    release()
    await expect(page.getByText('web compared')).toBeVisible()
    await expect(headerBadge(page, 'Synced')).toBeVisible()

    await page.getByRole('button', { name: 'Hard refresh' }).click()
    await expect(page.getByText('web rendered again and compared')).toBeVisible()
    expect(bodies).toEqual([{ hard: false }, { hard: true }])

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('Sync says what will happen, keeps a refusal in the dialog, and links to the execution it started', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'approver')
    await mockApplicationPage(page)
    const bodies: Schemas['SyncApplicationRequest'][] = []
    const sentence = 'The project shop is in observe mode. Switch it to execute mode to sync its applications.'
    let release: () => void = () => undefined
    const held = new Promise<void>((resolve) => (release = resolve))
    await on(page, 'POST', `/api/v1/applications/${APP_ID}/sync`, async (route, request) => {
      bodies.push(request.postDataJSON() as Schemas['SyncApplicationRequest'])
      if (bodies.length === 1) return route.fulfill(refusal(sentence))
      await held
      return route.fulfill(json({ executionId: 'e2e-exec-sync' } satisfies Schemas['OperationStarted'], 201))
    })

    await page.goto(`/applications/${APP_ID}`)
    await expect(page.getByRole('heading', { level: 1, name: 'web', exact: true })).toBeVisible({ timeout: 20_000 })

    await page.getByRole('button', { name: 'Sync', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: 'Sync web' })
    await expect(dialog.getByText(/previews the change, waits for an approval and a timed/)).toBeVisible()
    await expect(dialog.getByText('2 resources differ from the repository at')).toBeVisible()

    // Dry run turns it into a preview, and the dialog says so.
    await dialog.getByRole('checkbox', { name: 'Dry run' }).click()
    await expect(dialog.getByText(/only previews what a sync to production \/ shop would create, change or prune/)).toBeVisible()
    await expect(dialog.getByRole('button', { name: 'Start preview' })).toBeVisible()
    await dialog.getByRole('checkbox', { name: 'Dry run' }).click()
    await dialog.getByRole('checkbox', { name: 'Prune' }).click()

    // A 412 is the server's own sentence, kept in the dialog.
    await dialog.getByRole('button', { name: 'Request sync' }).click()
    await expect(dialog.getByRole('alert')).toHaveText(sentence)
    await expect(dialog).toBeVisible()

    const submit = dialog.getByRole('button', { name: 'Request sync' })
    await submit.click()
    await expect(submit).toHaveAttribute('aria-busy', 'true')
    await expect(submit.locator('[data-slot="spinner"]')).toBeVisible()
    release()

    await expect(dialog).toBeHidden()
    await expect(page.getByText('Sync of web requested')).toBeVisible()
    const notice = page.getByRole('status').filter({ hasText: 'Sync requested' })
    await expect(notice.getByRole('link', { name: 'Open the execution' })).toHaveAttribute('href', '/executions/e2e-exec-sync')
    expect(bodies).toEqual([
      { prune: true, dryRun: false },
      { prune: true, dryRun: false },
    ])

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('history names who synced, never by id; Roll back asks for an execution and links to it', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'approver')
    await mockApplicationPage(page, { application: application({ syncPolicy: { automated: true, selfHeal: true, prune: false, createNamespace: false, retryLimit: 0 } }) })
    const bodies: Schemas['RollbackApplicationRequest'][] = []
    await on(page, 'POST', `/api/v1/applications/${APP_ID}/rollback`, (route, request) => {
      bodies.push(request.postDataJSON() as Schemas['RollbackApplicationRequest'])
      return route.fulfill(json({ executionId: 'e2e-exec-rollback' } satisfies Schemas['OperationStarted'], 201))
    })

    await page.goto(`/applications/${APP_ID}`)
    const history = region(page, 'History')
    const rows = history.getByRole('list', { name: 'Sync history' }).getByRole('listitem')
    await expect(rows).toHaveCount(3, { timeout: 20_000 })

    await expect(rows.nth(0).getByText('Succeeded', { exact: true })).toBeVisible()
    await expect(rows.nth(0).getByText('octocat')).toBeVisible()
    await expect(rows.nth(0).getByRole('link', { name: 'Execution' })).toHaveAttribute('href', '/executions/e2e-exec-current')
    await expect(rows.nth(0).getByText('2m', { exact: true })).toBeVisible()
    await expect(rows.nth(1).getByText('Failed', { exact: true })).toBeVisible()
    await expect(rows.nth(1).getByText('Automated', { exact: true })).toBeVisible()
    await expect(rows.nth(1).getByText('Sync policy', { exact: true })).toBeVisible()
    await expect(rows.nth(1).getByText('Deployment shop/web: ImagePullBackOff')).toBeVisible()
    await expect(rows.nth(0).getByRole('link', { name: `Commit ${SYNCED_SHA.slice(0, 7)} on GitHub` })).toHaveAttribute(
      'href',
      `${REPO_URL}/commit/${SYNCED_SHA}`,
    )

    // The sync the cluster runs is where it already is; a failed one never deployed.
    await expectDisabledWithReason(page, rows.nth(0).getByRole('button', { name: /^Roll back/ }), 'The cluster already runs this sync.')
    await expectDisabledWithReason(page, rows.nth(1).getByRole('button', { name: /^Roll back/ }), 'Only a sync that succeeded can be rolled back to.')

    await rows.nth(2).getByRole('button', { name: `Roll back to ${OLDEST_SHA.slice(0, 7)}` }).click()
    const dialog = page.getByRole('alertdialog', { name: `Roll web back to ${OLDEST_SHA.slice(0, 7)}?` })
    await expect(dialog.getByText(/previews, waits for an approval and a timed/)).toBeVisible()
    await expect(dialog.getByText(/renders today’s source at/)).toBeVisible()
    // With self heal on, the rollback would not last: the dialog says so.
    await expect(dialog.getByText(/Automated sync with self heal is on/)).toBeVisible()
    await dialog.getByRole('button', { name: 'Request rollback' }).click()

    await expect(dialog).toBeHidden()
    const notice = page.getByRole('status').filter({ hasText: `Rollback to ${OLDEST_SHA.slice(0, 7)} requested` })
    await expect(notice.getByRole('link', { name: 'Open the execution' })).toHaveAttribute('href', '/executions/e2e-exec-rollback')
    expect(bodies).toEqual([{ historyId: 'h-oldest', prune: false }])

    await expectAccessible(page)
    await expectNothingPrivate(page)
    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('Delete needs the typed name, cascades only when asked, and returns to the project’s applications', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'admin')
    await mockApplicationPage(page)
    await on(page, 'GET', '/api/v1/applications', (route) => route.fulfill(json([])))
    const deletes: string[] = []
    await on(page, 'DELETE', `/api/v1/applications/${APP_ID}`, (route, request) => {
      deletes.push(new URL(request.url()).search)
      return route.fulfill(json({ deletedResources: 6 } satisfies Schemas['DeletedApplication']))
    })

    await page.goto(`/applications/${APP_ID}`)
    await expect(page.getByRole('heading', { level: 1, name: 'web', exact: true })).toBeVisible({ timeout: 20_000 })

    await page.getByRole('button', { name: 'Delete', exact: true }).click()
    const dialog = page.getByRole('alertdialog', { name: 'Delete web?' })
    const confirm = dialog.getByRole('button', { name: 'Delete application' })
    await expect(confirm).toBeDisabled()
    await dialog.getByLabel(/to confirm/).fill('we')
    await expect(confirm).toBeDisabled()
    await dialog.getByLabel(/to confirm/).fill('web')
    await expect(confirm).toBeEnabled()

    await dialog.getByRole('checkbox', { name: 'Also delete its resources from the cluster' }).click()
    await expect(dialog.getByText(/Deletes the 6 resources it manages from production \/ shop/)).toBeVisible()
    await dialog.getByRole('button', { name: 'Delete application and resources' }).click()

    await expect(page).toHaveURL(new RegExp(`/projects/${PROJECT_ID}/applications$`))
    await expect(page.getByText('6 resources were deleted from production / shop.')).toBeVisible()
    expect(deletes).toEqual(['?cascade=true'])

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('manifests are rendered only when asked for, highlighted, and windowed when long', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'viewer')
    const mock = await mockApplicationPage(page)
    // A large chart: 300 documents of eight lines each.
    const manifests = Array.from({ length: 300 }, (_, index) =>
      ['apiVersion: v1', 'kind: ConfigMap', 'metadata:', `  name: settings-${index}`, '  namespace: shop', 'data:', `  key: value-${index}`, '  mode: production'].join('\n'),
    )
    await on(page, 'GET', `/api/v1/applications/${APP_ID}/manifests`, (route) => {
      mock.manifestRequests += 1
      return route.fulfill(json({ manifests, revision: SHA } satisfies Schemas['Manifests']))
    })

    await page.goto(`/applications/${APP_ID}`)
    const frame = region(page, 'Manifests')
    await expect(frame.getByRole('button', { name: 'Show manifests' })).toBeVisible({ timeout: 20_000 })
    expect(mock.manifestRequests).toBe(0)

    await frame.getByRole('button', { name: 'Show manifests' }).click()
    await expect(frame.getByText('300 documents')).toBeVisible()
    await expect(frame.getByRole('link', { name: 'Commit 4f9c2ab on GitHub' })).toBeVisible()
    const yaml = frame.getByLabel(/^Rendered manifests/)
    await expect(yaml).toHaveAttribute('data-virtual', 'true')
    await expect(yaml.getByText('name: settings-0', { exact: true }).first()).toBeVisible()
    // Windowed: a fraction of the 2 699 lines is in the DOM.
    expect(await yaml.locator(':scope > div').count()).toBeLessThan(200)
    // The grammar arrives with the highlighter chunk; the lines then carry its tokens.
    await expect(yaml.locator('.code-token').first()).toBeVisible()
    await expect(frame.getByRole('button', { name: 'Copy the manifests' })).toBeVisible()
    expect(mock.manifestRequests).toBe(1)

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })
})

/* Workspace › Clusters */

test.describe('clusters', () => {
  test.beforeEach(requireApp)

  test('lists each cluster with its status, server, version, applications and check time, never a kubeconfig', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'admin')
    await on(page, 'GET', '/api/v1/clusters', (route) => route.fulfill(json(CLUSTERS)))

    await page.goto('/workspace/clusters')

    await expect(page.getByRole('heading', { level: 1, name: 'Clusters', exact: true })).toBeVisible({ timeout: 20_000 })
    await expect(page.getByText(/reads the kubeconfig in memory only, and never shows or returns it/)).toBeVisible()
    const rows = region(page, 'Clusters').getByRole('list', { name: 'Clusters' }).getByRole('listitem')
    await expect(rows).toHaveCount(3)
    // By name.
    await expect(rows.nth(0)).toContainText('edge')
    await expect(rows.nth(0).getByText('Unknown', { exact: true })).toBeVisible()
    await expect(rows.nth(0).getByText('checked never')).toBeVisible()
    await expect(rows.nth(1)).toContainText('production')
    await expect(rows.nth(1).getByText('Connected', { exact: true })).toBeVisible()
    await expect(rows.nth(1).getByText('https://k8s.production.acme.example:6443')).toBeVisible()
    await expect(rows.nth(1).getByText('v1.31.2')).toBeVisible()
    await expect(rows.nth(1).getByText('2 applications')).toBeVisible()
    await expect(rows.nth(1).getByText('KUBECONFIG_PRODUCTION')).toBeVisible()
    await expect(rows.nth(1).getByText('shop, payments')).toBeVisible()
    await expect(rows.nth(2).getByText('Unreachable', { exact: true })).toBeVisible()
    await expect(rows.nth(2).getByText('dial tcp 10.0.4.12:6443: i/o timeout')).toBeVisible()
    // The sidebar section is open on its page, next to Registries and Secrets.
    const main = page.getByRole('navigation', { name: 'Main' })
    await expect(main.getByRole('link', { name: 'Clusters', exact: true })).toBeVisible()

    await expectAccessible(page)
    await expectNothingPrivate(page)
    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('the editor names the kubeconfig secret, or stores a pasted one it never shows; a save reports an unreachable cluster', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'admin')
    await on(page, 'GET', '/api/v1/clusters', (route) => route.fulfill(json(CLUSTERS)))
    const workspaceId = mode === 'offline' ? OFFLINE_WORKSPACE_ID : null
    const secretsPath = /^\/api\/v1\/workspaces\/[^/]+\/secrets$/
    await on(page, 'GET', secretsPath, (route) =>
      route.fulfill(json([{ name: 'KUBECONFIG_STAGING', hint: 'ab12', description: null, updatedAt: ago(60), updatedBy: null }] satisfies Schemas['SecretInfo'][])),
    )
    const stored: { path: string; body: Schemas['SetSecretRequest'] }[] = []
    await on(page, 'PUT', /^\/api\/v1\/workspaces\/[^/]+\/secrets\/[^/]+$/, (route, request) => {
      const path = new URL(request.url()).pathname
      stored.push({ path, body: request.postDataJSON() as Schemas['SetSecretRequest'] })
      return route.fulfill(json({ name: path.split('/').at(-1) ?? '', hint: 'ab12', description: null, updatedAt: ago(0) } satisfies Schemas['SecretInfo']))
    })
    const saved: Schemas['UpsertClusterRequest'][] = []
    await on(page, 'POST', '/api/v1/clusters', (route, request) => {
      const body = request.postDataJSON() as Schemas['UpsertClusterRequest']
      saved.push(body)
      return route.fulfill(
        json(
          cluster({
            id: 'e2e-cluster-lab',
            name: body.name,
            kubeconfigSecret: body.kubeconfigSecret,
            status: 'unreachable',
            message: 'Get "https://k8s.lab.acme.example:6443/version": x509: certificate signed by unknown authority',
          }),
          201,
        ),
      )
    })

    await page.goto('/workspace/clusters')
    await page.getByRole('button', { name: 'Add cluster' }).first().click()
    const dialog = page.getByRole('dialog', { name: 'Add cluster' })
    await expect(dialog.getByText(/A kubeconfig is never typed here/)).toBeVisible()
    await dialog.getByLabel('Name', { exact: true }).fill('lab')

    // The names are listed; a new one is stored from a paste, and only its size is ever shown.
    await dialog.getByRole('combobox', { name: 'Kubeconfig secret' }).click()
    await expect(page.getByRole('option', { name: /^KUBECONFIG_STAGING/ })).toBeVisible()
    await page.keyboard.press('Escape')
    await dialog.getByRole('button', { name: 'Store a new secret' }).click()
    const store = dialog.getByRole('group', { name: 'Store a kubeconfig as a workspace secret' })
    await store.getByLabel('Secret name').fill('kubeconfig lab')
    await expect(store.getByLabel('Secret name')).toHaveValue('KUBECONFIG_LAB')
    const box = store.getByLabel('Kubeconfig')
    await box.evaluate((element, text) => {
      const data = new DataTransfer()
      data.setData('text/plain', text)
      element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }))
    }, KUBECONFIG)
    await expect(store.getByText(/Received \(pasted\): \d+ bytes, 15 lines\. It is not shown\./)).toBeVisible()
    await expect(box).toHaveValue('')
    await expectNothingPrivate(page)
    await store.getByRole('button', { name: 'Store secret' }).click()
    await expect(dialog.getByRole('combobox', { name: 'Kubeconfig secret' })).toContainText('KUBECONFIG_LAB')
    await expect(store).toBeHidden()

    // The kubeconfig went to the secret store once, every line intact.
    expect(stored).toHaveLength(1)
    expect(stored[0].path).toMatch(/\/secrets\/KUBECONFIG_LAB$/)
    if (workspaceId) expect(stored[0].path).toContain(`/workspaces/${workspaceId}/`)
    expect(stored[0].body.value).toBe(KUBECONFIG)

    await dialog.getByLabel('Default namespace').fill('lab')
    await dialog.getByLabel('Namespaces').fill('lab, lab-jobs ,')
    await dialog.getByRole('button', { name: 'Add cluster' }).click()

    await expect(page.getByText('lab added, but it did not answer')).toBeVisible()
    await expect(page.getByText(/x509: certificate signed by unknown authority/)).toBeVisible()
    expect(saved).toEqual([
      { name: 'lab', kubeconfigSecret: 'KUBECONFIG_LAB', context: null, defaultNamespace: 'lab', namespaces: ['lab', 'lab-jobs'] },
    ] satisfies Schemas['UpsertClusterRequest'][])

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })

  test('a cluster in use is not deleted: the server’s sentence stays in the dialog', async ({ page }) => {
    const guard = watchConsole(page)
    const leaked = await prepare(page, 'admin')
    await on(page, 'GET', '/api/v1/clusters', (route) => route.fulfill(json(CLUSTERS)))
    const sentence = 'production is used by 2 applications (web, api): delete them or move them to another cluster first.'
    await on(page, 'DELETE', `/api/v1/clusters/${CLUSTER_ID}`, (route) => route.fulfill(refusal(sentence)))

    await page.goto('/workspace/clusters')
    await page.getByRole('button', { name: 'Delete production' }).click()
    const dialog = page.getByRole('alertdialog', { name: 'Delete production?' })
    await expect(dialog.getByText(/2 applications deploy to it/)).toBeVisible()
    await dialog.getByRole('button', { name: 'Delete cluster' }).click()
    await expect(dialog.getByRole('alert')).toHaveText(sentence)
    await expect(dialog).toBeVisible()

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })
})

/* Against the running gateway */

test.describe('against the running gateway', () => {
  test('every region shows content or its own error, never a blank page', async ({ page }) => {
    test.skip(mode !== 'stack', 'needs the dev stack')
    const guard = watchConsole(page)
    const leaked = await forbidWrites(page)

    if (stackProject) {
      await page.goto(`/projects/${stackProject.id}/applications`)
      await expect(page.getByRole('heading', { level: 1, name: 'Applications', exact: true })).toBeVisible({ timeout: 20_000 })
      const panel = region(page, 'Applications')
      await expectRegionSettled(panel, panel.getByRole('table', { name: 'Applications' }).or(panel.getByText('No applications yet')))

      // An application, when there is one: its regions settle the same way.
      const first = panel.getByRole('table', { name: 'Applications' }).getByRole('link').first()
      if (await first.isVisible()) {
        await first.click()
        await expect(page.getByRole('heading', { level: 1 })).toBeVisible({ timeout: 20_000 })
        const resources = region(page, 'Resources')
        await expectRegionSettled(resources, resources.getByRole('heading', { level: 3 }).first().or(resources.getByText(/Nothing to manage|Not compared yet/)))
        const history = region(page, 'History')
        await expectRegionSettled(history, history.getByRole('list', { name: 'Sync history' }).or(history.getByText('No syncs yet')))
      }
    }

    await page.goto('/workspace/clusters')
    await expect(page.getByRole('heading', { level: 1, name: 'Clusters', exact: true })).toBeVisible({ timeout: 20_000 })
    const clusters = region(page, 'Clusters')
    await expectRegionSettled(clusters, clusters.getByRole('list', { name: 'Clusters' }).or(clusters.getByText('No clusters yet')))

    expect(leaked).toEqual([])
    guard.assertClean({ apiUp: true })
  })
})

/* Live invalidation: pure, no browser */

test.describe('live events', () => {
  const event = (action: string, status: string, entityId = APP_ID) => ({ entity: 'application', entityId, action, status, projectId: PROJECT_ID })

  test('an application event refreshes what the service stores, and its diff and manifests unless it is only progress', () => {
    expect(FAMILIES_BY_ENTITY.application).toEqual([['gitops']])
    for (const action of ['created', 'updated', 'deleted']) {
      expect(gatedFamilies(event(action, 'out_of_sync')), action).toEqual([
        ['gitops-diff', APP_ID],
        ['gitops-manifests', APP_ID],
      ])
    }
    // An operation's progress moves neither: the diff is read again when it ends.
    expect(gatedFamilies(event('progress', 'running'))).toEqual([])
  })
})
