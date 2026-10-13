/**
 * Which query families a platform event touches (CONTRACT.md §8, "Live
 * events"). Pure, so the mapping can be read and tested without the stream.
 *
 * Two lanes:
 *
 * - `FAMILIES_BY_ENTITY`: lists and details the services answer from their
 *   own store. Every event of the entity invalidates them, at most four
 *   times a second.
 * - `gatedFamilies`: the two answers the server has to go and get. Merge
 *   readiness reads GitHub on every request and the security overview is
 *   computed per project (CONTRACT.md §23), so only an event that can change
 *   them reaches them, scoped to the event's project, and the provider
 *   flushes that lane at most once a second.
 */
// Relative imports only: the Playwright suite imports this module as it is.
import type { components } from '../api/schema'
import type { Family } from './invalidation-coalescer'

export type PlatformEvent = components['schemas']['PlatformEvent']

/** Entity → query families to invalidate on every event of that entity. */
export const FAMILIES_BY_ENTITY: Record<string, readonly Family[]> = {
  project: [['projects'], ['scans']],
  scan: [['projects'], ['scans']],
  proposal: [['proposals'], ['decisions'], ['documents']],
  decision: [['proposals'], ['decisions'], ['documents']],
  document: [['proposals'], ['decisions'], ['documents']],
  execution: [['executions'], ['approvals']],
  approval: [['executions'], ['approvals']],
  pattern: [['patterns']],
  diagram: [['diagrams']],
  review: [['reviews'], ['pull-requests'], ['activity']],
  finding: [['reviews'], ['activity']],
  // Continuous delivery (CONTRACT.md §22): the board, the releases and the deployment history.
  deployment: [['delivery-cd', 'pipeline'], ['delivery-cd', 'runs'], ['delivery-cd', 'releases']],
  release: [['delivery-cd', 'pipeline'], ['delivery-cd', 'releases']],
  environment: [['delivery-cd', 'pipeline']],
}

/** Every family the stream can touch; refetched when the stream comes up or drops. */
export const ALL_FAMILIES: readonly Family[] = [
  ['projects'],
  ['scans'],
  ['proposals'],
  ['decisions'],
  ['documents'],
  ['executions'],
  ['approvals'],
  ['patterns'],
  ['diagrams'],
  ['reviews'],
  ['pull-requests'],
  ['activity'],
  ['delivery-cd'],
]

/** Roots of the gated lane; `features/reviews/api/keys.ts` and `features/security/api/keys.ts` build on them. */
export const MERGE_READINESS_ROOT = 'merge-readiness'
export const SECURITY_ROOT = 'security'

/** Both gated roots, for the one refetch after the stream reconnects. */
export const ALL_GATED_FAMILIES: readonly Family[] = [[MERGE_READINESS_ROOT], [SECURITY_ROOT]]

/** A review that is still reading or analyzing: the merge gate says "running" until it ends. */
const REVIEW_IN_FLIGHT: ReadonlySet<string> = new Set(['context', 'analyzing'])
/** An execution that is over: what it was going to change has changed, or never will. */
const EXECUTION_SETTLED: ReadonlySet<string> = new Set(['succeeded', 'failed', 'cancelled'])

/**
 * The gated families one event invalidates; none for most events.
 *
 * - `review`: its start and its end move the gate (`no_review`,
 *   `review_running`, `review_outdated`), as does a fix or merge execution
 *   attached to it. The steps in between do not.
 * - `finding`: a verdict can lift `findings_undecided`. A finding that is
 *   still pending (a fix was drafted for it) leaves the gate where it was.
 * - `execution`: one that was just created or has just ended. A fix commit
 *   moves the head, a merge closes the pull request, turning Dependabot on
 *   changes the repository's settings; the steps in between change nothing
 *   these answers carry. The reviews follow, since they name the execution.
 */
export function gatedFamilies(event: Pick<PlatformEvent, 'entity' | 'action' | 'status' | 'projectId'>): Family[] {
  const scope = event.projectId ? [event.projectId] : []
  const readiness: Family = [MERGE_READINESS_ROOT, ...scope]

  switch (event.entity) {
    case 'review':
      return event.action !== 'created' && REVIEW_IN_FLIGHT.has(event.status) ? [] : [readiness]
    case 'finding':
      return event.status === 'pending' ? [] : [readiness]
    case 'execution':
      return event.action === 'created' || EXECUTION_SETTLED.has(event.status)
        ? [readiness, [SECURITY_ROOT, ...scope], ['reviews']]
        : []
    default:
      return []
  }
}
