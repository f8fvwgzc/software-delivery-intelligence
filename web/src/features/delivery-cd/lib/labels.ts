import type { TagTone } from '@/components/system/tag'
import { formatDuration } from '@/features/delivery/lib/format'

import type { DeploymentKind, DeploymentRun, DeploymentRunStatus, PipelineStage, Registry, StageHealth } from '../api/types'

export { formatDuration, percent, shortSha } from '@/features/delivery/lib/format'

interface StatusMeta {
  label: string
  tone: TagTone
  /** Pulses the dot: something is in flight. */
  live?: boolean
}

export const DEPLOYMENT_STATUS: Record<DeploymentRunStatus, StatusMeta> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  pending: { label: 'Queued', tone: 'neutral', live: true },
  awaiting_approval: { label: 'Waiting for approval', tone: 'warning' },
  deploying: { label: 'Deploying', tone: 'brand', live: true },
  verifying: { label: 'Verifying', tone: 'brand', live: true },
  succeeded: { label: 'Succeeded', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
  cancelled: { label: 'Cancelled', tone: 'neutral' },
}

type RunState = Pick<DeploymentRun, 'status' | 'finishedAt'>

/**
 * A failed step pauses the run on its execution (retry, roll back, cancel;
 * or a missing permission to grant). The run then reads `failed` without a
 * `finishedAt`: it is not over, it waits for someone.
 */
export function needsAttention(run: RunState) {
  return run.status === 'failed' && !run.finishedAt
}

const NEEDS_ATTENTION: StatusMeta = { label: 'Needs attention', tone: 'warning' }

/** How a run reads on the board and in the history. A status a newer gateway adds still renders. */
export function runStatus(run: RunState): StatusMeta {
  if (needsAttention(run)) return NEEDS_ATTENTION
  return DEPLOYMENT_STATUS[run.status] ?? { label: run.status, tone: 'neutral' }
}

const IN_FLIGHT: ReadonlySet<DeploymentRunStatus> = new Set(['pending', 'awaiting_approval', 'deploying', 'verifying'])

/** The run is not over: in flight, waiting for an approval, or paused for attention. The board keeps watching it. */
export function isRunOpen(run: RunState) {
  return IN_FLIGHT.has(run.status) || needsAttention(run)
}

export const DEPLOYMENT_KIND: Record<DeploymentKind, StatusMeta> = {
  promote: { label: 'Promote', tone: 'neutral' },
  rollback: { label: 'Rollback', tone: 'warning' },
}

export function deploymentKind(kind: string): StatusMeta {
  return DEPLOYMENT_KIND[kind as DeploymentKind] ?? { label: kind, tone: 'neutral' }
}

export const STAGE_HEALTH: Record<StageHealth, StatusMeta> = {
  healthy: { label: 'Healthy', tone: 'success' },
  unhealthy: { label: 'Unhealthy', tone: 'destructive' },
  unknown: { label: 'Health unknown', tone: 'neutral' },
}

export function stageHealth(health: string): StatusMeta {
  return STAGE_HEALTH[health as StageHealth] ?? STAGE_HEALTH.unknown
}

/** `Release.source`: what created the release. */
const RELEASE_SOURCE: Record<string, string> = {
  pipeline: 'Build workflow',
  package: 'Registry package',
  manual: 'Manual',
}

export function releaseSource(source: string) {
  return RELEASE_SOURCE[source] ?? source
}

/** `ghcr.io/acme`: where a registry puts images, as one mono string. */
export function registryPath(registry: Pick<Registry, 'host' | 'namespace'>) {
  return registry.namespace ? `${registry.host}/${registry.namespace}` : registry.host
}

/**
 * Median lead time for people: minutes below an hour, hours below two
 * days, days above. (Zero means no release has reached the last stage yet;
 * the caller shows a dash for it.)
 */
export function formatLeadTime(seconds: number) {
  if (seconds < 48 * 3600) return formatDuration(seconds)
  const days = Math.floor(seconds / 86_400)
  const hours = Math.round((seconds % 86_400) / 3600)
  return hours > 0 ? `${days}d ${hours}h` : `${days}d`
}

/** Environment and deployer URLs are typed by people: only `http(s)` ones become links. */
export { safeHttpUrl } from '@/lib/urls'

/** `https://staging.example.com/app` → `staging.example.com`. */
export function hostOf(url: string) {
  try {
    return new URL(url).host
  } catch {
    return url
  }
}

/**
 * When the release a stage runs went live there: the end of the run that
 * put it there. A stage whose last run was something else (a failed
 * promotion of a newer release) has no such time on the board.
 */
export function deployedAt(stage: PipelineStage): string | null {
  const { current, lastRun } = stage
  if (!current || !lastRun) return null
  return lastRun.releaseId === current.id && lastRun.status === 'succeeded' ? (lastRun.finishedAt ?? null) : null
}
