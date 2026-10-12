export interface ExecutionListFilters {
  projectId?: string
  limit?: number
}

export const executionKeys = {
  all: ['executions'] as const,
  lists: () => [...executionKeys.all, 'list'] as const,
  list: (filters: ExecutionListFilters) => [...executionKeys.lists(), filters] as const,
  /** The paged table; under `lists()` so the same invalidations reach it. */
  pages: (filters: ExecutionListFilters) => [...executionKeys.lists(), 'pages', filters] as const,
  details: () => [...executionKeys.all, 'detail'] as const,
  detail: (executionId: string) => [...executionKeys.details(), executionId] as const,
  /**
   * Its own root, outside the `executions` family: the log is fed by the
   * execution's own SSE stream, so neither a platform event nor a mutation
   * may refetch the whole page of it.
   */
  events: (executionId: string) => ['execution-events', executionId] as const,
}
