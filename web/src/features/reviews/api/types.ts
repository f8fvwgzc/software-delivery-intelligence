import type { components } from '@/lib/api/schema'

/*
 * Review DTOs come from the gateway's OpenAPI contract (CONTRACT.md §20).
 * Only the string unions the schema leaves open (`state`, step keys and
 * statuses, activity actions, the publish event) are narrowed here.
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
