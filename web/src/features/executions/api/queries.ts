import { infiniteQueryOptions, queryOptions, type InfiniteData } from '@tanstack/react-query'

import { projectQueries } from '@/features/projects/api/queries'
import { api, unwrap } from '@/lib/api/client'
import { fallbackPoll } from '@/lib/live/connection-store'
import type { components } from '@/lib/api/schema'

import { isTerminal } from '../lib/labels'
import { executionKeys, type ExecutionListFilters } from './keys'

type Execution = components['schemas']['Execution']

const LIVE_INTERVAL = 3_000
/** Rows per page of the executions table; a full page means there may be an earlier one. */
export const EXECUTIONS_PAGE_SIZE = 50

function hasLiveExecution(executions: Execution[] | undefined) {
  return executions?.some((execution) => !isTerminal(execution.status)) ?? false
}

function hasLiveExecutionInPages(data: InfiniteData<Execution[]> | undefined) {
  return data?.pages.some(hasLiveExecution) ?? false
}

export const executionQueries = {
  /** Newest first. Polls every 3s while any execution is still running and the live stream is down. */
  list: (filters: ExecutionListFilters = {}) =>
    queryOptions({
      queryKey: executionKeys.list(filters),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/executions', { params: { query: filters }, signal })),
      refetchInterval: (query) => (hasLiveExecution(query.state.data) ? fallbackPoll(LIVE_INTERVAL) : false),
    }),

  /**
   * The executions table: newest first, paged backwards with `before` (the
   * oldest row's `updatedAt`), so a workspace with thousands of runs loads
   * one page and offers "Load earlier".
   */
  pages: (filters: ExecutionListFilters = {}) =>
    infiniteQueryOptions({
      queryKey: executionKeys.pages(filters),
      queryFn: ({ pageParam, signal }) =>
        unwrap(
          api.GET('/api/v1/executions', {
            params: { query: { ...filters, limit: EXECUTIONS_PAGE_SIZE, ...(pageParam ? { before: pageParam } : {}) } },
            signal,
          }),
        ),
      initialPageParam: '' as string,
      getNextPageParam: (lastPage) => (lastPage.length === EXECUTIONS_PAGE_SIZE ? lastPage[lastPage.length - 1].updatedAt : undefined),
      refetchInterval: (query) => (hasLiveExecutionInPages(query.state.data) ? fallbackPoll(LIVE_INTERVAL) : false),
    }),

  /** Steps and, while paused, the failure. Polls as a fallback to the SSE stream. */
  detail: (executionId: string) =>
    queryOptions({
      queryKey: executionKeys.detail(executionId),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/executions/{id}', { params: { path: { id: executionId } }, signal })),
      refetchInterval: (query) => {
        const status = query.state.data?.status
        return status && !isTerminal(status) ? fallbackPoll(LIVE_INTERVAL) : false
      },
    }),

  /** The initial page of the event log; the live tail comes over SSE. */
  events: (executionId: string) =>
    queryOptions({
      queryKey: executionKeys.events(executionId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/executions/{id}/events', { params: { path: { id: executionId }, query: { limit: 500 } }, signal })),
      staleTime: Infinity,
    }),

  /** Projects to pick from when creating a dry run: the projects feature's own list, so the request is shared. */
  projectOptions: () => queryOptions({ ...projectQueries.list(), staleTime: 60_000 }),
}
