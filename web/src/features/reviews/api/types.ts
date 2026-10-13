import type { components } from '@/lib/api/schema'

/*
 * Review DTOs come from the gateway's OpenAPI contract (CONTRACT.md §20, and
 * §23 for fixes and merging). Only the string unions the schema leaves open
 * (`state`, step keys and statuses, activity actions, the publish event,
 * who wrote a suggestion, merge blocker codes) are narrowed here.
 */

type Schemas = components['schemas']

export type PullRequestSummary = Schemas['PullRequestSummary']
export type PullRequestList = Schemas['PullRequestsResponse']
export type Review = Schemas['Review']
export type ReviewStep = Schemas['ReviewStep']
export type Finding = Schemas['Finding']
export type ReviewActivity = Schemas['ReviewActivity']
export type ReviewStatus = Schemas['ReviewStatus']
export type FindingSeverity = Schemas['FindingSeverity']
export type FindingDecision = Schemas['FindingDecision']
export type DecideFindingRequest = Schemas['DecideFindingRequest']
export type PublishReviewRequest = Schemas['PublishReviewRequest']
export type MergeReadiness = Schemas['MergeReadiness']
export type MergeBlocker = Schemas['MergeBlocker']
export type MergeMethod = Schemas['MergeMethod']
export type MergePullRequestRequest = Schemas['MergePullRequestRequest']
export type MergeResult = Schemas['MergeResult']

/** `PullRequestSummary.state` (proto: open | closed | merged | draft). */
export type PullRequestState = 'open' | 'closed' | 'merged' | 'draft'
/** `ReviewStep.key`, in pipeline order. */
export type ReviewStepKey = 'context' | 'analyze' | 'findings' | 'summary' | 'publish'
/** `ReviewStep.status`. */
export type ReviewStepStatus = 'pending' | 'running' | 'done' | 'failed'
/** `PublishReviewRequest.event`. */
export type ReviewEvent = 'comment' | 'request_changes' | 'approve'
/** `ReviewActivity.action`. */
export type ReviewActivityAction =
  | 'review.started'
  | 'review.proposed'
  | 'finding.accepted'
  | 'finding.dismissed'
  | 'review.published'
  | 'review.failed'
  | 'fix.drafted'
  | 'fixes.requested'
  | 'merge.requested'
/**
 * `Finding.suggestionSource` (CONTRACT.md §23.2): `rule` and `model` are exact
 * replacements for the finding's lines; `example` is the analyzer's
 * illustration to adapt by hand, which is never committed.
 */
export type SuggestionSource = 'rule' | 'model' | 'example'
/** `MergeBlocker.code` (CONTRACT.md §23.3). */
export type MergeBlockerCode =
  | 'not_open'
  | 'draft'
  | 'conflicts'
  | 'mergeability_unknown'
  | 'checks_failing'
  | 'checks_pending'
  | 'changes_requested'
  | 'blocked_by_github'
  | 'no_review'
  | 'review_outdated'
  | 'review_running'
  | 'findings_undecided'
  | 'merge_in_progress'
