export interface ExecutionListFilters {
  projectId?: string
  limit?: number
}

export const executionKeys = {
  all: ['executions'] as const,
  lists: () => [...executionKeys.all, 'list'] as const,
  list: (filters: ExecutionListFilters) => [...executionKeys.lists(), filters] as const,
  details: () => [...executionKeys.all, 'detail'] as const,
  detail: (executionId: string) => [...executionKeys.details(), executionId] as const,
  /** A sibling of `detail`, so refreshing the execution does not refetch the whole log. */
  events: (executionId: string) => [...executionKeys.all, 'events', executionId] as const,
  /** Projects offered in the "New dry run" dialog. */
  projectOptions: () => [...executionKeys.all, 'project-options'] as const,
}
