import type { StatusTone } from '@/components/shared/data/status-badge'
import type { components } from '@/lib/api/schema'

type ExecutionStatus = components['schemas']['ExecutionStatus']
type StepStatus = components['schemas']['StepStatus']
type RiskLevel = components['schemas']['RiskLevel']
type PolicyDecision = components['schemas']['PolicyDecision']
type EventType = components['schemas']['EventType']

interface StatusMeta {
  label: string
  tone: StatusTone
  /** Pulses the badge dot. */
  live?: boolean
}

export const EXECUTION_STATUS: Record<ExecutionStatus, StatusMeta> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  planning: { label: 'Planning', tone: 'neutral' },
  waiting_for_approval: { label: 'Waiting for approval', tone: 'warning' },
  authorized: { label: 'Authorized', tone: 'info' },
  executing: { label: 'Executing', tone: 'brand', live: true },
  verifying: { label: 'Verifying', tone: 'brand', live: true },
  paused: { label: 'Paused', tone: 'warning' },
  diagnosing: { label: 'Diagnosing', tone: 'warning', live: true },
  replanning: { label: 'Replanning', tone: 'info', live: true },
  succeeded: { label: 'Succeeded', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
  cancelled: { label: 'Cancelled', tone: 'neutral' },
}

const TERMINAL: ReadonlySet<ExecutionStatus> = new Set(['succeeded', 'failed', 'cancelled'])

export function isTerminal(status: ExecutionStatus) {
  return TERMINAL.has(status)
}

/** The execution is waiting on a remediation choice. */
export function isPausedOnFailure(status: ExecutionStatus) {
  return status === 'paused' || status === 'diagnosing'
}

export function canStart(status: ExecutionStatus) {
  return status === 'planning'
}

export function canCancel(status: ExecutionStatus) {
  return !isTerminal(status)
}

export const STEP_STATUS: Record<StepStatus, StatusMeta> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  pending: { label: 'Pending', tone: 'neutral' },
  waiting_approval: { label: 'Waiting for approval', tone: 'warning' },
  ready: { label: 'Ready', tone: 'info' },
  running: { label: 'Running', tone: 'brand', live: true },
  verifying: { label: 'Verifying', tone: 'brand', live: true },
  succeeded: { label: 'Succeeded', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
  blocked: { label: 'Blocked', tone: 'neutral' },
  skipped: { label: 'Skipped', tone: 'neutral' },
  cancelled: { label: 'Cancelled', tone: 'neutral' },
}

export const POLICY_DECISION: Record<PolicyDecision, string> = {
  unspecified: 'No policy',
  auto: 'Runs automatically',
  approval: 'Needs approval',
  required: 'Approval required',
  blocked: 'Blocked by policy',
}

export const EVENT_TYPE: Record<EventType, StatusMeta> = {
  unspecified: { label: 'event', tone: 'neutral' },
  state_changed: { label: 'state', tone: 'info' },
  policy_checked: { label: 'policy', tone: 'neutral' },
  permission_checked: { label: 'permission', tone: 'neutral' },
  permission_denied: { label: 'denied', tone: 'destructive' },
  approval_requested: { label: 'approval', tone: 'warning' },
  approval_decided: { label: 'decided', tone: 'info' },
  step_started: { label: 'started', tone: 'brand' },
  step_succeeded: { label: 'succeeded', tone: 'success' },
  step_failed: { label: 'failed', tone: 'destructive' },
  step_blocked: { label: 'blocked', tone: 'neutral' },
  checkpoint: { label: 'checkpoint', tone: 'success' },
  diagnosis: { label: 'diagnosis', tone: 'warning' },
  replanned: { label: 'replanned', tone: 'info' },
  log: { label: 'log', tone: 'neutral' },
}

export type Adapter = components['schemas']['Adapter']

/** The runtimes that exist (CONTRACT.md §5.4), in the order offered. */
export const RUNTIME_ADAPTERS = ['simulated', 'docker', 'github'] as const satisfies readonly Adapter[]

export function isRuntimeAdapter(value: string): value is Adapter {
  return (RUNTIME_ADAPTERS as readonly string[]).includes(value)
}

export interface AdapterChoice {
  value: Adapter
  label: string
  description: string
}

export const ADAPTER_CHOICES: readonly AdapterChoice[] = [
  {
    value: 'simulated',
    label: 'Dry run (simulated)',
    description: 'Timed dry run with optional fault injection. Never touches anything.',
  },
  {
    value: 'docker',
    label: 'Verify locally with Docker',
    description: 'Builds and runs the image on this machine, needs the local-docker identity.',
  },
  {
    value: 'github',
    label: 'Open pull request',
    description: 'Commits the generated files to a new branch and opens a PR, needs repository write.',
  },
]

/** Short chip text: `simulated` reads as "dry run"; the real runtimes are named as-is. */
const ADAPTER_CHIP: Record<Adapter, string> = { simulated: 'dry run', docker: 'docker', github: 'github' }

export function adapterLabel(adapter: string) {
  return isRuntimeAdapter(adapter) ? ADAPTER_CHIP[adapter] : adapter
}

/**
 * CONTRACT.md §5.4: the action types each runtime can run. `null` means every
 * action type (the simulated runtime refuses nothing).
 */
const ADAPTER_SUPPORT: Record<Adapter, ReadonlySet<string> | null> = {
  simulated: null,
  docker: new Set([
    'repo.checkout',
    'container.build',
    'container.run',
    'tests.run_local',
    'env.create_temporary',
    'env.destroy_temporary',
    'deps.install',
  ]),
  github: new Set(['repo.checkout', 'repo.open_pull_request']),
}

/**
 * Action types the chosen runtime cannot run, deduplicated, in plan order.
 * A plan may mix families: when the runtime is not `simulated`, a step that
 * names `docker` or `github` itself is routed to that family instead.
 */
export function unsupportedActions(runtime: Adapter, steps: { actionType: string; adapter: string }[]) {
  if (runtime === 'simulated') return []
  const unsupported = new Set<string>()
  for (const step of steps) {
    const family = step.adapter === 'docker' || step.adapter === 'github' ? step.adapter : runtime
    const support = ADAPTER_SUPPORT[family]
    if (support !== null && !support.has(step.actionType)) unsupported.add(step.actionType)
  }
  return [...unsupported]
}

/** Display labels for `Execution.outputs` keys; unknown keys fall back to the key itself. */
export const OUTPUT_LABEL: Record<string, string> = {
  pull_request_url: 'Pull request',
  image: 'Image',
  container_url: 'Container',
  workdir: 'Working directory',
}

/** CONTRACT.md §5.5 */
export const FAILURE_CODES = [
  'PERMISSION_DENIED',
  'POLICY_BLOCKED',
  'SKU_UNAVAILABLE',
  'QUOTA_EXCEEDED',
  'TIMEOUT',
  'HEALTH_CHECK_FAILED',
  'BUILD_FAILED',
  'TESTS_FAILED',
  'DEPENDENCY_CONFLICT',
  'UNKNOWN',
] as const

/** CONTRACT.md §5.2, in table order. */
export const ACTION_TYPES: { actionType: string; label: string; risk: RiskLevel }[] = [
  { actionType: 'repo.read', label: 'Read repository', risk: 'safe' },
  { actionType: 'repo.analyze', label: 'Analyze repository', risk: 'safe' },
  { actionType: 'tests.run_local', label: 'Run tests locally', risk: 'safe' },
  { actionType: 'docs.generate', label: 'Generate documentation', risk: 'safe' },
  { actionType: 'deps.install', label: 'Install dependencies', risk: 'medium' },
  { actionType: 'container.build', label: 'Build container image', risk: 'medium' },
  { actionType: 'container.run', label: 'Run container', risk: 'medium' },
  { actionType: 'env.create_temporary', label: 'Create temporary environment', risk: 'medium' },
  { actionType: 'db.create_temporary', label: 'Create temporary database', risk: 'medium' },
  { actionType: 'cloud.create_dev_resource', label: 'Create development cloud resource', risk: 'medium' },
  { actionType: 'cloud.modify_resource', label: 'Modify existing cloud resource', risk: 'high' },
  { actionType: 'deploy.staging', label: 'Deploy to staging', risk: 'high' },
  { actionType: 'deploy.production', label: 'Deploy to production', risk: 'high' },
  { actionType: 'db.migrate_production', label: 'Run production database migration', risk: 'high' },
  { actionType: 'backup.restore_production', label: 'Restore production backup', risk: 'high' },
  { actionType: 'resource.delete', label: 'Delete resource', risk: 'high' },
  { actionType: 'firewall.modify', label: 'Modify firewall', risk: 'high' },
  { actionType: 'secrets.rotate', label: 'Rotate secrets', risk: 'high' },
  { actionType: 'db.destroy_production', label: 'Destroy production database', risk: 'blocked' },
  { actionType: 'repo.checkout', label: 'Download the repository at a commit', risk: 'safe' },
  { actionType: 'env.destroy_temporary', label: 'Remove a temporary environment', risk: 'safe' },
  { actionType: 'repo.open_pull_request', label: 'Open a pull request with generated files', risk: 'medium' },
]

/** CONTRACT.md §5.4 */
export const ADAPTERS = ['simulated', 'github', 'docker', 'cloud', 'kubernetes', 'local'] as const

/** Milliseconds between two ISO timestamps, formatted for a mono cell. */
export function formatDuration(startedAt?: string | null, finishedAt?: string | null) {
  if (!startedAt) return null
  const start = Date.parse(startedAt)
  const end = finishedAt ? Date.parse(finishedAt) : Date.now()
  if (Number.isNaN(start) || Number.isNaN(end)) return null
  const ms = Math.max(0, end - start)
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const minutes = Math.floor(ms / 60_000)
  const seconds = Math.round((ms % 60_000) / 1000)
  return `${minutes}m ${seconds.toString().padStart(2, '0')}s`
}
