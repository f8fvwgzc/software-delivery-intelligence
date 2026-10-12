import { infiniteQueryOptions, queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import { fallbackPoll } from '@/lib/live/connection-store'

import { isReviewRunning } from '../lib/labels'
import { activityKeys, pullRequestKeys, reviewKeys, type ActivityFilters, type PullRequestFilter, type ReviewListFilters } from './keys'

/** Fallback cadence while a review runs and the live stream is down. */
const RUNNING_INTERVAL = 3_000
export const ACTIVITY_PAGE_SIZE = 50
export const REVIEWS_LIMIT = 50
export const PULL_REQUESTS_LIMIT = 50

export const pullRequestQueries = {
  /** The project's pull requests from GitHub, cached by the knowledge service; `refresh` re-reads them. */
  list: (projectId: string, state: PullRequestFilter, refresh = false) =>
    queryOptions({
      queryKey: pullRequestKeys.list(projectId, state),
      queryFn: ({ signal }) =>
        unwrap(
          api.GET('/api/v1/projects/{id}/pull-requests', {
            params: { path: { id: projectId }, query: { state, limit: PULL_REQUESTS_LIMIT, ...(refresh ? { refresh: true } : {}) } },
            signal,
          }),
        ),
      staleTime: 60_000,
    }),
}

export const reviewQueries = {
  /** Summaries only, newest first. */
  list: (filters: ReviewListFilters = {}) =>
    queryOptions({
      queryKey: reviewKeys.list(filters),
      queryFn: ({ signal }) =>
        unwrap(
          api.GET('/api/v1/reviews', {
            params: { query: { limit: REVIEWS_LIMIT, ...(filters.projectId ? { projectId: filters.projectId } : {}) } },
            signal,
          }),
        ),
      refetchInterval: (query) =>
        query.state.data?.some((review) => isReviewRunning(review.status)) ? fallbackPoll(RUNNING_INTERVAL) : false,
    }),

  /** Steps, findings and summary. Live through the `review` entity on the platform stream; polls only as a fallback. */
  detail: (reviewId: string) =>
    queryOptions({
      queryKey: reviewKeys.detail(reviewId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/reviews/{id}', { params: { path: { id: reviewId } }, signal })),
      refetchInterval: (query) => {
        const status = query.state.data?.status
        return status && isReviewRunning(status) ? fallbackPoll(RUNNING_INTERVAL) : false
      },
    }),
}

export const activityQueries = {
  /** The review loop's audit, newest first, paged backwards by `before`. */
  list: (filters: ActivityFilters = {}) =>
    infiniteQueryOptions({
      queryKey: activityKeys.list(filters),
      queryFn: ({ pageParam, signal }) =>
        unwrap(
          api.GET('/api/v1/activity', {
            params: {
              query: {
                limit: ACTIVITY_PAGE_SIZE,
                ...(filters.projectId ? { projectId: filters.projectId } : {}),
                ...(pageParam ? { before: pageParam } : {}),
              },
            },
            signal,
          }),
        ),
      initialPageParam: '' as string,
      getNextPageParam: (lastPage) => (lastPage.length === ACTIVITY_PAGE_SIZE ? lastPage[lastPage.length - 1].at : undefined),
      staleTime: 30_000,
    }),
}
