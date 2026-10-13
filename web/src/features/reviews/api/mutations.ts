import { useMutation, useQueryClient } from '@tanstack/react-query'

import { approvalKeys } from '@/features/approvals/api/keys'
import { executionKeys } from '@/features/executions/api/keys'
import { api, unwrap } from '@/lib/api/client'
import { liveConnection } from '@/lib/live/connection-store'

import { activityKeys, mergeKeys, pullRequestKeys, reviewKeys } from './keys'
import type { DecideFindingRequest, MergeMethod, MergeReadiness, PublishReviewRequest, Review } from './types'

/*
 * A refusal of these requests is a sentence that says what to do (which
 * switch to turn on, which head to review first). It is shown where the
 * person asked, so the mutations opt out of the global error toast
 * (`meta.silent`, see `lib/query-client.ts`).
 */
const INLINE_ERRORS = { silent: true } as const

/** Starts a review of a pull request at its current head; answers the new (or running) review. */
export function useStartReview(projectId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (number: number) =>
      unwrap(api.POST('/api/v1/projects/{id}/pull-requests/{number}/reviews', { params: { path: { id: projectId, number } } })),
    onSuccess: (review) => {
      queryClient.setQueryData(reviewKeys.detail(review.id), review)
      queryClient.invalidateQueries({ queryKey: reviewKeys.lists() })
      queryClient.invalidateQueries({ queryKey: pullRequestKeys.all })
      queryClient.invalidateQueries({ queryKey: activityKeys.all })
    },
  })
}

export interface DecideFindingVariables extends DecideFindingRequest {
  findingId: string
}

/** Accepts or dismisses one finding; the review's counters change too. */
export function useDecideFinding(reviewId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ findingId, decision, note }: DecideFindingVariables) =>
      unwrap(
        api.POST('/api/v1/findings/{id}/decide', {
          params: { path: { id: findingId } },
          body: { decision, note: note?.trim() || null },
        }),
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: reviewKeys.detail(reviewId) })
      queryClient.invalidateQueries({ queryKey: reviewKeys.lists() })
      queryClient.invalidateQueries({ queryKey: activityKeys.all })
      // A verdict can lift the merge gate. The live stream tells the readiness
      // query (lib/live/event-families.ts); only without it does the mutation.
      if (!liveConnection.isConnected()) queryClient.invalidateQueries({ queryKey: mergeKeys.all })
    },
  })
}

/**
 * `POST /findings/{id}/draft-fix` (member): asks the workspace's model for
 * the replacement of the lines the finding points at. The answer is the
 * finding with its suggestion; its decision does not change.
 */
export function useDraftFix(reviewId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (findingId: string) =>
      unwrap(api.POST('/api/v1/findings/{id}/draft-fix', { params: { path: { id: findingId } } })),
    meta: INLINE_ERRORS,
    onSuccess: (finding) => {
      queryClient.setQueryData<Review>(reviewKeys.detail(reviewId), (review) =>
        review ? { ...review, findings: review.findings.map((row) => (row.id === finding.id ? finding : row)) } : review,
      )
      queryClient.invalidateQueries({ queryKey: activityKeys.all })
    },
  })
}

/** A request that writes to the repository became an execution, which may open an approval. */
function useStartedExecution() {
  const queryClient = useQueryClient()
  return () => {
    queryClient.invalidateQueries({ queryKey: executionKeys.lists() })
    queryClient.invalidateQueries({ queryKey: approvalKeys.all })
    queryClient.invalidateQueries({ queryKey: activityKeys.all })
  }
}

/**
 * `POST /reviews/{id}/apply-fixes` (approver): one execution that commits
 * the suggestion of every accepted finding to the pull request's branch.
 * The answer is the review, whose `fixExecutionId` is that execution.
 */
export function useApplyFixes(reviewId: string) {
  const queryClient = useQueryClient()
  const onStarted = useStartedExecution()
  return useMutation({
    mutationFn: () => unwrap(api.POST('/api/v1/reviews/{id}/apply-fixes', { params: { path: { id: reviewId } } })),
    meta: INLINE_ERRORS,
    onSuccess: (review) => {
      queryClient.setQueryData(reviewKeys.detail(reviewId), review)
      queryClient.invalidateQueries({ queryKey: reviewKeys.lists() })
      onStarted()
    },
  })
}

export interface MergeVariables {
  method: MergeMethod
  deleteBranch: boolean
}

interface MergeTarget {
  projectId: string
  prNumber: number
  /** The review the merge frame sits on; it names the merge execution afterwards. */
  reviewId: string
}

/**
 * `POST /projects/{id}/pull-requests/{number}/merge` (approver): one
 * execution that verifies the head again, merges exactly that commit and,
 * when asked, deletes the branch. Refused with the first blocker's sentence
 * unless the pull request is ready.
 */
export function useMergePullRequest({ projectId, prNumber, reviewId }: MergeTarget) {
  const queryClient = useQueryClient()
  const onStarted = useStartedExecution()
  const key = mergeKeys.readiness(projectId, prNumber)
  return useMutation({
    mutationFn: ({ method, deleteBranch }: MergeVariables) =>
      unwrap(
        api.POST('/api/v1/projects/{id}/pull-requests/{number}/merge', {
          params: { path: { id: projectId, number: prNumber } },
          body: { method, deleteBranch },
        }),
      ),
    meta: INLINE_ERRORS,
    onSuccess: (result) => {
      // The server just named the execution, so the frame can link to it at
      // once; the refetch brings the blocker that goes with it.
      queryClient.setQueryData<MergeReadiness>(key, { ...result.readiness, mergeExecutionId: result.executionId })
      queryClient.invalidateQueries({ queryKey: key })
      queryClient.invalidateQueries({ queryKey: reviewKeys.detail(reviewId) })
      queryClient.invalidateQueries({ queryKey: reviewKeys.lists() })
      onStarted()
    },
    // Refused: something changed since GitHub was last read. Read it again.
    onError: () => queryClient.invalidateQueries({ queryKey: key }),
  })
}

/** Posts the review to GitHub: the summary as the body, one comment per accepted finding. */
export function usePublishReview(reviewId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: PublishReviewRequest) =>
      unwrap(api.POST('/api/v1/reviews/{id}/publish', { params: { path: { id: reviewId } }, body })),
    onSuccess: (review) => {
      queryClient.setQueryData(reviewKeys.detail(reviewId), review)
      queryClient.invalidateQueries({ queryKey: reviewKeys.lists() })
      queryClient.invalidateQueries({ queryKey: pullRequestKeys.all })
      queryClient.invalidateQueries({ queryKey: activityKeys.all })
    },
  })
}
