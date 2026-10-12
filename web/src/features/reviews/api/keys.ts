import type { PullRequestState } from './types'

export type PullRequestFilter = 'open' | 'closed' | 'all'

export interface ReviewListFilters {
  projectId?: string
  limit?: number
}

export interface ActivityFilters {
  projectId?: string
}

/*
 * Three families, so the live stream can target them: `review` events
 * invalidate all three, `finding` events the reviews and the activity.
 */
export const pullRequestKeys = {
  all: ['pull-requests'] as const,
  list: (projectId: string, state: PullRequestFilter) => [...pullRequestKeys.all, 'list', projectId, state] as const,
  detail: (projectId: string, number: number) => [...pullRequestKeys.all, 'detail', projectId, number] as const,
}

export const reviewKeys = {
  all: ['reviews'] as const,
  lists: () => [...reviewKeys.all, 'list'] as const,
  list: (filters: ReviewListFilters) => [...reviewKeys.lists(), filters] as const,
  details: () => [...reviewKeys.all, 'detail'] as const,
  detail: (reviewId: string) => [...reviewKeys.details(), reviewId] as const,
}

export const activityKeys = {
  all: ['activity'] as const,
  list: (filters: ActivityFilters) => [...activityKeys.all, 'list', filters] as const,
}

export type { PullRequestState }
