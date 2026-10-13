import type { TagTone } from '@/components/system/tag'
import { count } from '@/lib/intl'

import type { MergeBlockerCode, MergeMethod, MergeReadiness } from '../api/types'
import { prState } from './labels'

/*
 * Pure readers of a `MergeReadiness` (CONTRACT.md §23.3): what the merge
 * frame shows as tags, when it asks GitHub again, and which methods it
 * offers. Who may ask for what is in `permissions.ts`.
 */

/** How often the frame asks again while GitHub is still working something out. */
export const RECHECK_INTERVAL_MS = 10_000

export const MERGE_METHOD: Record<MergeMethod, { label: string; description: string }> = {
  squash: { label: 'Squash and merge', description: 'One commit on the base branch' },
  merge: { label: 'Merge commit', description: 'Every commit, joined by a merge commit' },
  rebase: { label: 'Rebase and merge', description: 'Every commit, replayed on the base branch' },
}

/** The order the server picks in when no method is named: squash, else merge, else rebase. */
const METHOD_PREFERENCE: readonly MergeMethod[] = ['squash', 'merge', 'rebase']

/** The methods the repository allows, in the server's order of preference. A method this build does not know is left out. */
export function allowedMethods(readiness: Pick<MergeReadiness, 'allowedMethods'>): MergeMethod[] {
  return METHOD_PREFERENCE.filter((method) => readiness.allowedMethods.includes(method))
}

const hasBlocker = (readiness: Pick<MergeReadiness, 'blockers'>, code: MergeBlockerCode) =>
  readiness.blockers.some((blocker) => blocker.code === code)

/** GitHub has not finished: mergeability is still being computed, or a check is still running. Worth asking again. */
export function needsRecheck(readiness: Pick<MergeReadiness, 'blockers'> | undefined) {
  if (!readiness) return false
  return hasBlocker(readiness, 'mergeability_unknown') || hasBlocker(readiness, 'checks_pending')
}

/** Closed or merged: there is nothing left to merge, so the frame offers no controls. */
export function isClosed(readiness: Pick<MergeReadiness, 'blockers'>) {
  return hasBlocker(readiness, 'not_open')
}

const BLOCKER_TONE: Record<MergeBlockerCode, TagTone> = {
  not_open: 'neutral',
  draft: 'neutral',
  conflicts: 'destructive',
  mergeability_unknown: 'warning',
  checks_failing: 'destructive',
  checks_pending: 'warning',
  changes_requested: 'destructive',
  blocked_by_github: 'warning',
  no_review: 'warning',
  review_outdated: 'warning',
  review_running: 'brand',
  findings_undecided: 'warning',
  merge_in_progress: 'brand',
}

/** Dot colour of a blocker; a code a newer gateway adds reads neutral. */
export function blockerTone(code: string): TagTone {
  return BLOCKER_TONE[code as MergeBlockerCode] ?? 'neutral'
}

export interface ReadinessTag {
  key: string
  label: string
  tone: TagTone
  /** Outline only: a fact, not a state that needs attention. */
  quiet?: boolean
  /** Pulses the dot: GitHub is still working on it. */
  live?: boolean
  title?: string
}

/** GitHub's side of the answer as tags: the pull request's state, its checks, its reviews, its branch. */
export function readinessTags(readiness: MergeReadiness): ReadinessTag[] {
  const tags: ReadinessTag[] = []

  if (readiness.state !== 'open') {
    const state = prState(readiness.state)
    tags.push({ key: 'state', label: state.label, tone: state.tone })
  }

  if (readiness.checksTotal === 0) {
    tags.push({ key: 'checks', label: 'No checks', tone: 'neutral', quiet: true, title: 'The head commit has no check runs or commit statuses' })
  } else {
    if (readiness.checksFailed > 0) {
      tags.push({ key: 'checks-failed', label: count(readiness.checksFailed, 'check failed', 'checks failed'), tone: 'destructive' })
    }
    if (readiness.checksPending > 0) {
      tags.push({ key: 'checks-pending', label: count(readiness.checksPending, 'check pending', 'checks pending'), tone: 'warning', live: true })
    }
    if (readiness.checksFailed === 0 && readiness.checksPending === 0) {
      tags.push({
        key: 'checks',
        label: 'Checks passed',
        tone: 'success',
        title: `${count(readiness.checksPassed, 'check', 'checks')} of ${readiness.checksTotal}`,
      })
    }
  }

  if (readiness.changesRequested > 0) {
    tags.push({ key: 'changes-requested', label: 'Changes requested', tone: 'destructive', title: count(readiness.changesRequested, 'reviewer', 'reviewers') })
  }
  tags.push(
    readiness.approvals > 0
      ? { key: 'approvals', label: count(readiness.approvals, 'approval', 'approvals'), tone: 'success' }
      : { key: 'approvals', label: 'No approvals', tone: 'neutral', quiet: true },
  )

  if (readiness.mergeable === 'conflicting') {
    tags.push({ key: 'conflicts', label: 'Conflicts', tone: 'destructive', title: `The branch conflicts with ${readiness.baseRef}` })
  } else if (readiness.mergeable === 'unknown') {
    tags.push({ key: 'conflicts', label: 'Mergeability unknown', tone: 'warning', live: true, title: 'GitHub is still computing it' })
  } else {
    tags.push({ key: 'conflicts', label: 'No conflicts', tone: 'neutral', quiet: true })
  }

  if (readiness.behindBy > 0) {
    tags.push({
      key: 'behind',
      label: `Behind by ${readiness.behindBy}`,
      tone: 'neutral',
      title: `${count(readiness.behindBy, 'commit', 'commits')} behind ${readiness.baseRef}`,
    })
  }
  if (readiness.baseProtected) {
    tags.push({ key: 'protected', label: 'Protected base', tone: 'neutral', quiet: true, title: `${readiness.baseRef} is protected: GitHub enforces its rules at merge time` })
  }
  if (readiness.isFork) {
    tags.push({ key: 'fork', label: 'Fork', tone: 'neutral', quiet: true, title: 'The branch lives in another repository' })
  }
  if (readiness.isDependabot) {
    tags.push({ key: 'dependabot', label: 'Dependabot', tone: 'neutral', quiet: true, title: 'Opened by Dependabot' })
  }
  return tags
}
