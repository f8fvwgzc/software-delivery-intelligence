import { routes } from '@/app/router/routes'
import type { StatusTone } from '@/components/shared/data'

import type { Approval, Execution, Project } from '../api/types'
import {
  APPROVAL_STATUS,
  EXECUTION_STATUS,
  failureLabel,
  isPausedOnFailure,
  plural,
  QUESTION_TONE,
  QUEUE_KIND,
  RISK,
  type QueueKind,
} from './labels'

export type StepState = 'done' | 'failed' | 'blocked' | 'pending'

export interface QueueRow {
  /** Stable across refetches: `<kind>:<source id>`. */
  id: string
  kind: QueueKind
  /** The row's own status tone, so it matches the entity's page. */
  tone: StatusTone
  live: boolean
  projectId: string
  projectName: string
  /** What the human has to do, in one line. */
  needed: string
  /** Which thing it is about. */
  detail: string
  /** When it started waiting; rows are sorted oldest first. */
  since: string | null
  /** Where the action happens. */
  to: string
  /** Inline rail for execution rows. */
  steps?: StepState[]
}

/** Per-step state of an execution for the inline rail. */
export function stepStates(execution: Execution): StepState[] {
  const failedKey = execution.failure?.stepKey ?? null
  const blocked = new Set(execution.failure?.blockedSteps ?? [])

  return execution.steps.map((step) => {
    if (step.checkpointed) return 'done'
    if (step.key === failedKey) return 'failed'
    if (blocked.has(step.key)) return 'blocked'
    return 'pending'
  })
}

function approvalRow(approval: Approval): QueueRow {
  return {
    id: `approval:${approval.id}`,
    kind: 'approval',
    tone: APPROVAL_STATUS[approval.status].tone,
    live: false,
    projectId: approval.projectId,
    projectName: approval.projectName,
    needed: `Approve ${approval.stepTitle}`,
    detail: `#${approval.executionNumber} ${approval.executionTitle} · ${RISK[approval.risk].label.toLowerCase()}`,
    since: approval.requestedAt,
    to: routes.approvals(),
  }
}

function pausedRow(execution: Execution): QueueRow {
  const failure = execution.failure ?? null
  const options = failure?.options.length ?? 0
  const status = EXECUTION_STATUS[execution.status]

  return {
    id: `paused:${execution.id}`,
    kind: 'paused',
    tone: status.tone,
    live: status.live ?? false,
    projectId: execution.projectId,
    projectName: execution.projectName,
    needed: failure
      ? options > 0
        ? `Pick a remediation (${plural(options, 'option')})`
        : 'Give an instruction to replan'
      : 'Diagnosing the failure',
    detail: `#${execution.number} ${execution.title}${failure ? ` · ${failureLabel(failure.code)}` : ''}`,
    since: failure?.occurredAt ?? execution.updatedAt,
    to: routes.execution(execution.id),
    steps: stepStates(execution),
  }
}

function questionRow(project: Project): QueueRow {
  return {
    id: `question:${project.id}`,
    kind: 'question',
    tone: QUESTION_TONE,
    live: false,
    projectId: project.id,
    projectName: project.name,
    needed: `Answer ${plural(project.openQuestions, 'open question')}`,
    detail: project.tldr || project.repoFullName,
    since: project.lastScannedAt ?? project.createdAt,
    to: routes.project(project.id),
  }
}

function compareRows(a: QueueRow, b: QueueRow) {
  const byKind = QUEUE_KIND[a.kind].order - QUEUE_KIND[b.kind].order
  if (byKind !== 0) return byKind
  return (a.since ?? '').localeCompare(b.since ?? '')
}

interface QueueInput {
  projects: Project[]
  approvals: Approval[]
  executions: Execution[]
}

/**
 * Everything waiting on the human: paused executions first (the run is
 * stopped), then pending approvals, then projects with open questions.
 * Within a kind, oldest first.
 */
export function buildWorkQueue({ projects, approvals, executions }: QueueInput): QueueRow[] {
  const rows: QueueRow[] = [
    ...executions.filter((execution) => isPausedOnFailure(execution.status)).map(pausedRow),
    ...approvals.filter((approval) => approval.status === 'pending').map(approvalRow),
    ...projects.filter((project) => project.openQuestions > 0).map(questionRow),
  ]
  return rows.sort(compareRows)
}

export type QueueFilter = 'all' | QueueKind

export function countByKind(rows: QueueRow[]): Record<QueueFilter, number> {
  const counts: Record<QueueFilter, number> = { all: rows.length, paused: 0, approval: 0, question: 0 }
  for (const row of rows) counts[row.kind] += 1
  return counts
}
