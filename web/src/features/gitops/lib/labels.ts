import type { TagTone } from '@/components/system/tag'

import type {
  ApplicationOperation,
  ClusterStatus,
  HealthStatus,
  OperationKind,
  OperationPhase,
  SourceType,
  SyncPolicy,
  SyncStatus,
} from '../api/types'

export { formatDuration, shortSha } from '@/features/delivery/lib/format'

export interface StatusMeta {
  label: string
  tone: TagTone
  /** Pulses the dot: something is in flight. */
  live?: boolean
  /** One sentence for a tooltip or a screen reader. */
  hint?: string
}

/** `SyncStatus` (CONTRACT.md §24.4). An application is synced, out of sync or unknown; a resource can also be missing or extraneous. */
export const SYNC_STATUS: Record<SyncStatus, StatusMeta> = {
  synced: { label: 'Synced', tone: 'success', hint: 'The cluster matches the repository.' },
  out_of_sync: { label: 'Out of sync', tone: 'warning', hint: 'The cluster differs from the repository.' },
  missing: { label: 'Missing', tone: 'warning', hint: 'Desired, not in the cluster yet: a sync creates it.' },
  extraneous: { label: 'Extraneous', tone: 'neutral', hint: 'In the cluster, no longer desired: a sync with prune deletes it.' },
  unknown: { label: 'Sync unknown', tone: 'neutral', hint: 'The last comparison did not finish.' },
}

export function syncStatus(status: string | null | undefined): StatusMeta {
  return SYNC_STATUS[status as SyncStatus] ?? SYNC_STATUS.unknown
}

/** `HealthStatus`: Argo CD's checks; an application's is the worst of its resources'. */
export const HEALTH_STATUS: Record<HealthStatus, StatusMeta> = {
  healthy: { label: 'Healthy', tone: 'success' },
  progressing: { label: 'Progressing', tone: 'info', live: true },
  degraded: { label: 'Degraded', tone: 'destructive' },
  suspended: { label: 'Suspended', tone: 'neutral' },
  missing: { label: 'Missing', tone: 'warning' },
  unknown: { label: 'Health unknown', tone: 'neutral' },
}

export function healthStatus(status: string | null | undefined): StatusMeta {
  return HEALTH_STATUS[status as HealthStatus] ?? HEALTH_STATUS.unknown
}

export const CLUSTER_STATUS: Record<ClusterStatus, StatusMeta> = {
  connected: { label: 'Connected', tone: 'success' },
  unreachable: { label: 'Unreachable', tone: 'destructive' },
  unknown: { label: 'Unknown', tone: 'neutral', hint: 'Not reached yet.' },
}

export function clusterStatus(status: string | null | undefined): StatusMeta {
  return CLUSTER_STATUS[status as ClusterStatus] ?? CLUSTER_STATUS.unknown
}

export const OPERATION_PHASE: Record<OperationPhase, StatusMeta> = {
  running: { label: 'Running', tone: 'info', live: true },
  terminating: { label: 'Terminating', tone: 'warning', live: true },
  succeeded: { label: 'Succeeded', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
  error: { label: 'Error', tone: 'destructive' },
  unknown: { label: 'Unknown', tone: 'neutral' },
}

export function operationPhase(phase: string | null | undefined): StatusMeta {
  return OPERATION_PHASE[phase as OperationPhase] ?? OPERATION_PHASE.unknown
}

const OPERATION_KIND: Record<OperationKind, string> = { sync: 'Sync', rollback: 'Rollback', unknown: 'Operation' }

export function operationKind(kind: string | null | undefined): string {
  return OPERATION_KIND[kind as OperationKind] ?? OPERATION_KIND.unknown
}

/** An operation that has not ended: the page keeps watching it, and a second one would be refused. */
export function isOperationRunning(operation: Pick<ApplicationOperation, 'phase' | 'finishedAt'> | null | undefined): boolean {
  if (!operation) return false
  return (operation.phase === 'running' || operation.phase === 'terminating') && !operation.finishedAt
}

/** `SourceType`, and `null`: decided when the source renders. */
export const SOURCE_TYPE: Record<SourceType, { label: string; description: string }> = {
  directory: { label: 'Directory', description: 'Every .yaml, .yml and .json file under the path' },
  kustomize: { label: 'Kustomize', description: 'kustomization.yaml at the path, with image overrides' },
  helm: { label: 'Helm', description: 'Chart.yaml at the path, with values files and parameters' },
}

export const DETECT_SOURCE = {
  label: 'Detect',
  description: 'kustomization.yaml makes it Kustomize, Chart.yaml Helm, anything else a directory',
}

export function sourceTypeLabel(type: SourceType | null | undefined): string {
  return type ? (SOURCE_TYPE[type]?.label ?? type) : 'Detected'
}

/** The automation tags of a sync policy, in the order Argo CD shows them. Empty when nothing syncs on its own. */
export function automationTags(policy: Pick<SyncPolicy, 'automated' | 'selfHeal' | 'prune'>): string[] {
  if (!policy.automated) return []
  return ['Auto sync', ...(policy.selfHeal ? ['Self heal'] : []), ...(policy.prune ? ['Prune'] : [])]
}

/** `cluster / namespace`, or the cluster alone when the application uses the cluster's default namespace. */
export function destinationLabel(clusterName: string, namespace: string | null | undefined): string {
  return namespace ? `${clusterName} / ${namespace}` : clusterName
}

/** Whoever started an operation: a user id, or `system` for the sync policy. */
export const SYSTEM_INITIATOR = 'system'
