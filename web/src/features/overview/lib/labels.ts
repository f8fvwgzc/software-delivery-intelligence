import type { StatusTone } from '@/components/shared/data'
import { APPROVAL_STATUS } from '@/features/approvals/lib/labels'
import { EXECUTION_STATUS, isPausedOnFailure, isTerminal } from '@/features/executions/lib/labels'
import { isProposalClosed, PROPOSAL_KIND_LABEL, PROPOSAL_STATUS } from '@/features/proposals/lib/labels'

import type { ExecutionStatus, ProposalStatus } from '../api/types'

/*
 * Status vocabularies belong to the features that own the entities. The
 * overview re-exports them so a paused execution looks the same here as on
 * the execution page, and so its components keep feature-local imports.
 */
export { APPROVAL_STATUS, EXECUTION_STATUS, PROPOSAL_KIND_LABEL, PROPOSAL_STATUS, isPausedOnFailure }
export { RISK } from '@/components/shared/data'

/** Failure codes (CONTRACT.md §5.5) in plain words. */
const FAILURE_CODE: Record<string, string> = {
  PERMISSION_DENIED: 'Permission denied',
  POLICY_BLOCKED: 'Blocked by policy',
  SKU_UNAVAILABLE: 'SKU unavailable',
  QUOTA_EXCEEDED: 'Quota exceeded',
  TIMEOUT: 'Timed out',
  HEALTH_CHECK_FAILED: 'Health check failed',
  BUILD_FAILED: 'Build failed',
  TESTS_FAILED: 'Tests failed',
  DEPENDENCY_CONFLICT: 'Dependency conflict',
  UNKNOWN: 'Unknown failure',
}

export function failureLabel(code: string) {
  return FAILURE_CODE[code] ?? code
}

/** Running or waiting on the machine: started, not finished, not stopped on a failure. */
export function isActiveExecution(status: ExecutionStatus) {
  return status !== 'planning' && status !== 'unspecified' && !isTerminal(status) && !isPausedOnFailure(status)
}

/** Still open for negotiation, acceptance or rejection. */
export function isOpenProposal(status: ProposalStatus) {
  return status !== 'unspecified' && !isProposalClosed(status)
}

export type QueueKind = 'paused' | 'approval' | 'question'

/** Work queue row kinds, in the order they are listed. The dot tone comes from the row's own status. */
export const QUEUE_KIND: Record<QueueKind, { label: string; action: string; order: number }> = {
  paused: { label: 'Paused', action: 'Resolve', order: 0 },
  approval: { label: 'Approval', action: 'Decide', order: 1 },
  question: { label: 'Question', action: 'Answer', order: 2 },
}

export const QUEUE_KINDS: QueueKind[] = ['paused', 'approval', 'question']

/** Open questions have no status object of their own; they read as information to supply. */
export const QUESTION_TONE: StatusTone = 'info'

/** 6px status dot fills. */
export const DOT_CLASS: Record<StatusTone, string> = {
  neutral: 'bg-muted-foreground/50',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
}

export function plural(count: number, singular: string, pluralForm = `${singular}s`) {
  return `${count.toLocaleString()} ${count === 1 ? singular : pluralForm}`
}
