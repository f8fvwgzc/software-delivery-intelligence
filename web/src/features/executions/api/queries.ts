import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

import { isTerminal } from '../lib/labels'
import { executionKeys, type ExecutionListFilters } from './keys'

type Execution = components['schemas']['Execution']

const LIVE_INTERVAL = 3_000

function hasLiveExecution(executions: Execution[] | undefined) {
  return executions?.some((execution) => !isTerminal(execution.status)) ?? false
}

export const executionQueries = {
  /** Newest first. Polls every 3s while any execution is still running. */
  list: (filters: ExecutionListFilters = {}) =>
    queryOptions({
      queryKey: executionKeys.list(filters),
      queryFn: () => unwrap(api.GET('/api/v1/executions', { params: { query: filters } })),
      refetchInterval: (query) => (hasLiveExecution(query.state.data) ? LIVE_INTERVAL : false),
    }),

  /** Steps and, while paused, the failure. Polls as a fallback to the SSE stream. */
  detail: (executionId: string) =>
    queryOptions({
      queryKey: executionKeys.detail(executionId),
      queryFn: () => unwrap(api.GET('/api/v1/executions/{id}', { params: { path: { id: executionId } } })),
      refetchInterval: (query) => {
        const status = query.state.data?.status
        return status && !isTerminal(status) ? LIVE_INTERVAL : false
      },
    }),

  /** The initial page of the event log; the live tail comes over SSE. */
  events: (executionId: string) =>
    queryOptions({
      queryKey: executionKeys.events(executionId),
      queryFn: () =>
        unwrap(api.GET('/api/v1/executions/{id}/events', { params: { path: { id: executionId }, query: { limit: 500 } } })),
      staleTime: Infinity,
    }),

  /** Projects to pick from when creating a dry run. */
  projectOptions: () =>
    queryOptions({
      queryKey: executionKeys.projectOptions(),
      queryFn: () => unwrap(api.GET('/api/v1/projects')),
      staleTime: 60_000,
    }),
}
