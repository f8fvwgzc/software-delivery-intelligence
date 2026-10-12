import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { activityKeys, pullRequestKeys, reviewKeys } from './keys'
import type { DecideFindingRequest, PublishReviewRequest } from './types'

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
    },
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
