/*
 * Typed helpers over the gateway's REST API (CONTRACT.md §8), used by the
 * specs to create and clean up fixtures without driving the UI for every
 * step. Types come from the generated schema, so a renamed field fails
 * `pnpm e2e:typecheck` instead of a run.
 */
import type { APIRequestContext } from '@playwright/test'

import type { components } from '../../src/lib/api/schema'

type Schemas = components['schemas']

export type Project = Schemas['Project']
export type Scan = Schemas['Scan']
export type Fact = Schemas['Fact']
export type FactKind = Schemas['FactKind']
export type Understanding = Schemas['Understanding']
export type MaturityScore = Schemas['MaturityScore']
export type Proposal = Schemas['Proposal']
export type ProposalStatus = Schemas['ProposalStatus']
export type NegotiateResponse = Schemas['NegotiateResponse']
export type Execution = Schemas['Execution']
export type ExecutionStatus = Schemas['ExecutionStatus']
export type ExecutionEvent = Schemas['ExecutionEvent']
export type PlanStep = Schemas['PlanStep']
export type FaultInjection = Schemas['FaultInjection']
export type Approval = Schemas['Approval']
export type ApprovalStatus = Schemas['ApprovalStatus']
export type Policy = Schemas['Policy']
export type PolicyDecision = Schemas['PolicyDecision']
export type Pattern = Schemas['Pattern']
export type SystemStatus = Schemas['SystemStatus']
export type GithubAccount = Schemas['GithubAccount']
export type Health = Schemas['Health']

type ErrorBody = Schemas['ErrorBody']
type CreateExecutionRequest = Schemas['CreateExecutionRequest']
type GenerateProposalRequest = Schemas['GenerateProposalRequest']
type AcceptProposalRequest = Schemas['AcceptProposalRequest']
type ResolveFailureRequest = Schemas['ResolveFailureRequest']
type DecideApprovalRequest = Schemas['DecideApprovalRequest']

/** A non-2xx answer, carrying the gateway's `{ code, message }` body. */
export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, call: string, body?: Partial<ErrorBody>) {
    super(`${call} answered ${status} ${body?.code ?? 'unknown'}: ${body?.message ?? 'no error body'}`)
    this.name = 'ApiError'
    this.status = status
    this.code = body?.code ?? 'unknown'
  }
}

type Method = 'GET' | 'POST' | 'PATCH' | 'DELETE'
type Query = Record<string, string | number | boolean | undefined>

function withQuery(path: string, query?: Query) {
  if (!query) return path
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined && value !== '') params.set(key, String(value))
  }
  const encoded = params.toString()
  return encoded ? `${path}?${encoded}` : path
}

async function call<T>(request: APIRequestContext, method: Method, path: string, body?: unknown): Promise<T> {
  const response = await request.fetch(`/api/v1${path}`, {
    method,
    data: body,
    headers: { accept: 'application/json' },
    failOnStatusCode: false,
  })
  if (!response.ok()) {
    let parsed: Partial<ErrorBody> | undefined
    try {
      parsed = (await response.json()) as Partial<ErrorBody>
    } catch {
      parsed = undefined
    }
    throw new ApiError(response.status(), `${method} ${path}`, parsed)
  }
  const text = await response.text()
  return (text ? JSON.parse(text) : undefined) as T
}

/* Gateway */

export const getHealth = (request: APIRequestContext) => call<Health>(request, 'GET', '/health')
export const getSystemStatus = (request: APIRequestContext) => call<SystemStatus>(request, 'GET', '/system/status')
export const getGithubAccount = (request: APIRequestContext) => call<GithubAccount>(request, 'GET', '/github/account')

/* Projects and knowledge */

export const listProjects = (request: APIRequestContext) => call<Project[]>(request, 'GET', '/projects')
export const getProject = (request: APIRequestContext, id: string) => call<Project>(request, 'GET', `/projects/${id}`)
export const connectProject = (request: APIRequestContext, repoFullName: string) =>
  call<Project>(request, 'POST', '/projects', { repoFullName })
export const deleteProject = (request: APIRequestContext, id: string) =>
  call<unknown>(request, 'DELETE', `/projects/${id}`)
export const getScan = (request: APIRequestContext, id: string) => call<Scan>(request, 'GET', `/scans/${id}`)
export const getUnderstanding = (request: APIRequestContext, projectId: string) =>
  call<Understanding>(request, 'GET', `/projects/${projectId}/understanding`)
export const listFacts = (request: APIRequestContext, projectId: string, kind?: FactKind) =>
  call<Fact[]>(request, 'GET', withQuery(`/projects/${projectId}/facts`, { kind }))
export const getMaturity = (request: APIRequestContext, projectId: string) =>
  call<MaturityScore>(request, 'GET', `/projects/${projectId}/maturity`)
export const listPatterns = (request: APIRequestContext) =>
  call<Pattern[]>(request, 'GET', withQuery('/patterns', { includeDeprecated: true }))
export const getPattern = (request: APIRequestContext, id: string) => call<Pattern>(request, 'GET', `/patterns/${id}`)

/* Proposals */

export const listProposals = (request: APIRequestContext, projectId?: string) =>
  call<Proposal[]>(request, 'GET', withQuery('/proposals', { projectId }))
export const getProposal = (request: APIRequestContext, id: string) => call<Proposal>(request, 'GET', `/proposals/${id}`)
export const createProposal = (request: APIRequestContext, body: GenerateProposalRequest) =>
  call<Proposal>(request, 'POST', '/proposals', body)
export const negotiate = (request: APIRequestContext, id: string, content: string) =>
  call<NegotiateResponse>(request, 'POST', `/proposals/${id}/messages`, { content })
export const acceptProposal = (request: APIRequestContext, id: string, body: AcceptProposalRequest = {}) =>
  call<Proposal>(request, 'POST', `/proposals/${id}/accept`, body)
export const rejectProposal = (request: APIRequestContext, id: string, reason: string) =>
  call<Proposal>(request, 'POST', `/proposals/${id}/reject`, { reason })

/* Executions and approvals */

export const listExecutions = (request: APIRequestContext, query: { projectId?: string; limit?: number } = {}) =>
  call<Execution[]>(request, 'GET', withQuery('/executions', query))
export const getExecution = (request: APIRequestContext, id: string) =>
  call<Execution>(request, 'GET', `/executions/${id}`)
export const createExecution = (request: APIRequestContext, body: CreateExecutionRequest) =>
  call<Execution>(request, 'POST', '/executions', body)
export const startExecution = (request: APIRequestContext, id: string, fault: FaultInjection | null = null) =>
  call<Execution>(request, 'POST', `/executions/${id}/start`, { fault })
export const resolveFailure = (request: APIRequestContext, id: string, body: ResolveFailureRequest) =>
  call<Execution>(request, 'POST', `/executions/${id}/resolve`, body)
export const cancelExecution = (request: APIRequestContext, id: string, reason: string) =>
  call<Execution>(request, 'POST', `/executions/${id}/cancel`, { reason })
export const listEvents = (request: APIRequestContext, id: string, afterSeq?: number) =>
  call<ExecutionEvent[]>(request, 'GET', withQuery(`/executions/${id}/events`, { afterSeq, limit: 500 }))
export const listApprovals = (request: APIRequestContext, query: { status?: ApprovalStatus; executionId?: string } = {}) =>
  call<Approval[]>(request, 'GET', withQuery('/approvals', query))
export const decideApproval = (request: APIRequestContext, id: string, body: DecideApprovalRequest) =>
  call<Approval>(request, 'POST', `/approvals/${id}/decide`, body)

/* Policies */

export const listPolicies = (request: APIRequestContext) => call<Policy[]>(request, 'GET', '/policies')
export const updatePolicy = (request: APIRequestContext, actionType: string, decision: PolicyDecision) =>
  call<Policy>(request, 'PATCH', `/policies/${encodeURIComponent(actionType)}`, { decision })

/* State helpers (CONTRACT.md §7) */

const TERMINAL: ReadonlySet<ExecutionStatus> = new Set(['succeeded', 'failed', 'cancelled'])
const CLOSED: ReadonlySet<ProposalStatus> = new Set(['accepted', 'rejected', 'superseded'])

export const isTerminal = (status: ExecutionStatus) => TERMINAL.has(status)
export const isPausedOnFailure = (status: ExecutionStatus) => status === 'paused' || status === 'diagnosing'
export const isProposalClosed = (status: ProposalStatus) => CLOSED.has(status)

/* Polling */

export interface PollOptions {
  timeoutMs?: number
  intervalMs?: number
  /** Named in the timeout error. */
  label?: string
  /** Keep polling through probe errors until the deadline (a starting stack). */
  tolerateErrors?: boolean
}

export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

/** Calls `probe` until `done` accepts its value, then returns that value. */
export async function poll<T>(
  probe: () => Promise<T>,
  done: (value: T) => boolean,
  { timeoutMs = 30_000, intervalMs = 1_000, label = 'the condition', tolerateErrors = false }: PollOptions = {},
): Promise<T> {
  const deadline = Date.now() + timeoutMs
  let last: { value: T } | undefined
  let lastError: unknown
  for (;;) {
    try {
      const value = await probe()
      last = { value }
      if (done(value)) return value
    } catch (error) {
      if (!tolerateErrors) throw error
      lastError = error
    }
    if (Date.now() >= deadline) {
      const detail = last
        ? `last value: ${JSON.stringify(last.value)?.slice(0, 400)}`
        : `last error: ${lastError instanceof Error ? lastError.message : String(lastError)}`
      throw new Error(`Timed out after ${timeoutMs}ms waiting for ${label} (${detail})`)
    }
    await sleep(intervalMs)
  }
}

/** Resolves once the scan settled; throws with the scan's error text when it failed. */
export async function waitForProjectReady(request: APIRequestContext, projectId: string, timeoutMs = 90_000) {
  const project = await poll(
    () => getProject(request, projectId),
    (current) => current.status === 'ready' || current.status === 'failed',
    { timeoutMs, intervalMs: 2_000, label: `project ${projectId} to finish scanning` },
  )
  if (project.status === 'failed') {
    const scan = project.lastScanId ? await getScan(request, project.lastScanId) : null
    throw new Error(`The scan of ${project.repoFullName} failed: ${scan?.error ?? 'no error text recorded'}`)
  }
  return project
}

/* Fixtures */

/** CONTRACT.md §5.3: the permissions each action type needs by default. */
const PERMISSIONS: Record<string, string[]> = {
  'repo.read': ['repository:read'],
  'repo.analyze': ['repository:read'],
  'tests.run_local': [],
  'docs.generate': ['repository:read'],
}

function step(key: string, title: string, actionType: string, dependsOn: string[] = []): PlanStep {
  return {
    key,
    title,
    description: title,
    actionType,
    adapter: 'simulated',
    dependsOn,
    params: {},
    requiredPermissions: PERMISSIONS[actionType] ?? [],
    verification: 'The adapter reports success and the step output is non-empty.',
  }
}

/** Three safe, auto-approved steps: a dry run of this plan succeeds without a human. */
export function quietPlan(): PlanStep[] {
  return [
    step('read', 'Read repository', 'repo.read'),
    step('analyze', 'Analyze repository', 'repo.analyze', ['read']),
    step('tests', 'Run tests locally', 'tests.run_local', ['analyze']),
  ]
}

/** A dry-run execution fixture in `planning`; cancel it with `cancelExecutionQuietly`. */
export function createDryRun(request: APIRequestContext, projectId: string, title = `e2e dry run ${new Date().toISOString()}`) {
  return createExecution(request, { projectId, title, steps: quietPlan(), adapter: 'simulated' })
}

/** Cancels an execution that is still running; silent when it is already over or gone. */
export async function cancelExecutionQuietly(request: APIRequestContext, id: string) {
  try {
    const execution = await getExecution(request, id)
    if (!isTerminal(execution.status)) await cancelExecution(request, id, 'e2e cleanup')
  } catch {
    // Already terminal, already gone, or the stack went away: nothing to undo.
  }
}

/** Rejects a proposal that is still open; silent when it is closed or gone. */
export async function rejectProposalQuietly(request: APIRequestContext, id: string) {
  try {
    const proposal = await getProposal(request, id)
    if (!isProposalClosed(proposal.status)) await rejectProposal(request, id, 'e2e cleanup')
  } catch {
    // Already closed, already gone, or the stack went away: nothing to undo.
  }
}
