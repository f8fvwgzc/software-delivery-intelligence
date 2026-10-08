import type { Approval, Execution, Project, Proposal } from '../api/types'
import { isActiveExecution, isOpenProposal, isPausedOnFailure, plural } from './labels'

export interface WaitingSummary {
  total: number
  approvals: number
  paused: number
  questions: number
}

export interface OverviewMetrics {
  waiting: WaitingSummary
  projectsReady: number
  projectsTotal: number
  executionsActive: number
  proposalsOpen: number
}

interface MetricsInput {
  projects: Project[]
  approvals: Approval[]
  executions: Execution[]
  proposals: Proposal[]
}

/** The hero and its supports, computed from the same lists the queue shows. */
export function computeOverviewMetrics({ projects, approvals, executions, proposals }: MetricsInput): OverviewMetrics {
  const pending = approvals.filter((approval) => approval.status === 'pending').length
  const paused = executions.filter((execution) => isPausedOnFailure(execution.status)).length
  const questions = projects.reduce((sum, project) => sum + project.openQuestions, 0)

  return {
    waiting: { total: pending + paused + questions, approvals: pending, paused, questions },
    projectsReady: projects.filter((project) => project.status === 'ready').length,
    projectsTotal: projects.length,
    executionsActive: executions.filter((execution) => isActiveExecution(execution.status)).length,
    proposalsOpen: proposals.filter((proposal) => isOpenProposal(proposal.status)).length,
  }
}

/** "2 approvals · 1 paused execution · 3 open questions", or a calm line when empty. */
export function describeWaiting(waiting: WaitingSummary) {
  if (waiting.total === 0) return 'Nothing is waiting on you'

  const parts = [
    waiting.approvals > 0 ? plural(waiting.approvals, 'approval') : null,
    waiting.paused > 0 ? plural(waiting.paused, 'paused execution') : null,
    waiting.questions > 0 ? plural(waiting.questions, 'open question') : null,
  ].filter(Boolean)

  return parts.join(' · ')
}
