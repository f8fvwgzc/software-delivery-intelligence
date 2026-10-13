import type { Role } from '@/features/auth/api/types'
import { hasRole } from '@/features/auth/lib/roles'

import type { MergeReadiness } from '../api/types'

/*
 * Role gates of the review page (CONTRACT.md §18, §20, §23.6). Each answers
 * the sentence shown next to a control that is off, or null when it is live.
 * The gateway refuses the calls anyway; these keep a person from pressing a
 * button that can only fail.
 */

/** `POST /findings/{id}/decide` is a member's request (CONTRACT.md §20). */
export function decideGate(role: Role): string | null {
  return hasRole(role, 'member') ? null : 'Deciding a finding needs the member role.'
}

/** `POST /findings/{id}/draft-fix` is a member's request. */
export function draftFixGate(role: Role): string | null {
  return hasRole(role, 'member') ? null : 'Drafting a fix needs the member role.'
}

/** `POST /reviews/{id}/apply-fixes` writes to the repository: approver. A fork's branch is not SDI's to commit to. */
export function applyFixesGate(role: Role, pullRequest?: Pick<MergeReadiness, 'isFork'>): string | null {
  if (!hasRole(role, 'approver')) return 'Applying fixes needs the approver role.'
  if (pullRequest?.isFork) {
    return 'This pull request comes from a fork, so its branch cannot be committed to. Publish the review and the author can apply the suggestions.'
  }
  return null
}

/** `POST .../merge` writes to the repository: approver, and only while nothing blocks. */
export function mergeGate(role: Role, readiness: Pick<MergeReadiness, 'canMerge' | 'blockers'>): string | null {
  if (!hasRole(role, 'approver')) return 'Merging needs the approver role.'
  if (!readiness.canMerge) return readiness.blockers[0]?.message ?? 'This pull request cannot be merged yet.'
  return null
}
