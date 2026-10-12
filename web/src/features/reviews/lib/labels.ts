import type { RailStatus } from '@/components/system/rail'
import type { TagTone } from '@/components/system/tag'

import type {
  FindingDecision,
  FindingSeverity,
  PullRequestState,
  ReviewActivityAction,
  ReviewEvent,
  ReviewStatus,
  ReviewStepKey,
  ReviewStepStatus,
} from '../api/types'

interface StatusMeta {
  label: string
  tone: TagTone
  live?: boolean
}

export const PR_STATE: Record<PullRequestState, StatusMeta> = {
  open: { label: 'Open', tone: 'success' },
  draft: { label: 'Draft', tone: 'neutral' },
  merged: { label: 'Merged', tone: 'brand' },
  closed: { label: 'Closed', tone: 'destructive' },
}

export const REVIEW_STATUS: Record<ReviewStatus, StatusMeta> = {
  unspecified: { label: 'Unknown', tone: 'neutral' },
  context: { label: 'Reading the PR', tone: 'brand', live: true },
  analyzing: { label: 'Analyzing', tone: 'brand', live: true },
  proposed: { label: 'Findings ready', tone: 'warning' },
  published: { label: 'Published', tone: 'success' },
  failed: { label: 'Failed', tone: 'destructive' },
}

export function isReviewRunning(status: ReviewStatus) {
  return status === 'context' || status === 'analyzing'
}

/** Findings can be decided only while the review waits for the human. */
export function canDecide(status: ReviewStatus) {
  return status === 'proposed'
}

export function canPublish(status: ReviewStatus) {
  return status === 'proposed'
}

/** The five steps in order; a review that has not reported a step yet still draws it. */
export const REVIEW_STEPS: { key: ReviewStepKey; label: string; hint: string }[] = [
  { key: 'context', label: 'Context', hint: 'Files, diff, touched facts and impacted graph nodes' },
  { key: 'analyze', label: 'Analyze', hint: 'Deterministic analyzers over the diff' },
  { key: 'findings', label: 'Findings', hint: 'Accept or dismiss each one' },
  { key: 'summary', label: 'Summary', hint: 'Walkthrough: intent, changes, impact, risk' },
  { key: 'publish', label: 'Publish', hint: 'A GitHub review with one comment per accepted finding' },
]

const STEP_RAIL_STATUS: Record<ReviewStepStatus, RailStatus> = {
  pending: 'pending',
  running: 'running',
  done: 'done',
  failed: 'failed',
}

/** `ReviewStep.status` is an open string in the contract; unknown values read as pending. */
export function stepRailStatus(status: string): RailStatus {
  return STEP_RAIL_STATUS[status as ReviewStepStatus] ?? 'pending'
}

/** `PullRequestSummary.state` is an open string; unknown values read as open. */
export function prState(state: string): StatusMeta {
  return PR_STATE[state as PullRequestState] ?? PR_STATE.open
}

export const SEVERITY: Record<FindingSeverity, StatusMeta & { rank: number }> = {
  critical: { label: 'Critical', tone: 'destructive', rank: 0 },
  high: { label: 'High', tone: 'destructive', rank: 1 },
  medium: { label: 'Medium', tone: 'warning', rank: 2 },
  low: { label: 'Low', tone: 'info', rank: 3 },
  info: { label: 'Info', tone: 'neutral', rank: 4 },
  unspecified: { label: 'Unrated', tone: 'neutral', rank: 5 },
}

/** Severities in display order, worst first. */
export const SEVERITY_ORDER: FindingSeverity[] = ['critical', 'high', 'medium', 'low', 'info', 'unspecified']

export const DECISION: Record<FindingDecision, StatusMeta> = {
  unspecified: { label: 'Pending', tone: 'neutral' },
  pending: { label: 'Pending', tone: 'neutral' },
  accepted: { label: 'Accepted', tone: 'success' },
  dismissed: { label: 'Dismissed', tone: 'neutral' },
}

export const REVIEW_EVENT: Record<ReviewEvent, { label: string; description: string }> = {
  comment: { label: 'Comment', description: 'Post without a verdict' },
  request_changes: { label: 'Request changes', description: 'Block merging until addressed' },
  approve: { label: 'Approve', description: 'Approve the pull request' },
}

export const ACTIVITY_ACTION: Record<ReviewActivityAction, StatusMeta> = {
  'review.started': { label: 'Started', tone: 'brand' },
  'review.proposed': { label: 'Proposed', tone: 'warning' },
  'finding.accepted': { label: 'Accepted', tone: 'success' },
  'finding.dismissed': { label: 'Dismissed', tone: 'neutral' },
  'review.published': { label: 'Published', tone: 'success' },
  'review.failed': { label: 'Failed', tone: 'destructive' },
}

/** Unknown actions (a newer gateway) still render. */
export function activityMeta(action: string): StatusMeta {
  return ACTIVITY_ACTION[action as ReviewActivityAction] ?? { label: action, tone: 'neutral' }
}

export function shortSha(sha: string) {
  return sha.slice(0, 7)
}

/** `path:line` or `path:line-endLine`. */
export function formatLocation(path: string, line: number, endLine: number) {
  if (!line) return path
  return endLine && endLine !== line ? `${path}:${line}-${endLine}` : `${path}:${line}`
}

export function formatDurationMs(ms: number) {
  if (!ms) return ''
  if (ms < 1000) return `${ms} ms`
  return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`
}

/** Avatar for a GitHub login; `system` and empty actors get none. */
export function actorAvatarUrl(actor: string) {
  if (!actor || actor === 'system' || actor.includes(' ')) return undefined
  return `https://github.com/${encodeURIComponent(actor)}.png?size=48`
}
