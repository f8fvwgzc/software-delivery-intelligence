import type { Execution, Proposal } from '../api/types'

export const RECENT_LIMIT = 5

function byUpdatedDesc(a: { updatedAt: string }, b: { updatedAt: string }) {
  return b.updatedAt.localeCompare(a.updatedAt)
}

/** Module-level so TanStack memoises the `select` result across renders. */
export function selectRecentExecutions(executions: Execution[]) {
  return executions.toSorted(byUpdatedDesc).slice(0, RECENT_LIMIT)
}

export function selectRecentProposals(proposals: Proposal[]) {
  return proposals.toSorted(byUpdatedDesc).slice(0, RECENT_LIMIT)
}
