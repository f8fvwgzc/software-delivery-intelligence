/** Query key factory for proposals and their negotiation threads. */
export const proposalKeys = {
  all: ['proposals'] as const,
  lists: () => [...proposalKeys.all, 'list'] as const,
  list: (filters: { projectId?: string }) => [...proposalKeys.lists(), filters] as const,
  /** The paged table; under `lists()` so the same invalidations reach it. */
  pages: (filters: { projectId?: string }) => [...proposalKeys.lists(), 'pages', filters] as const,
  details: () => [...proposalKeys.all, 'detail'] as const,
  detail: (id: string) => [...proposalKeys.details(), id] as const,
  messages: (id: string) => [...proposalKeys.detail(id), 'messages'] as const,
  /** Generated files of one option; a child of `detail` so a re-rank refreshes them too. */
  artifacts: (id: string, optionId: string) => [...proposalKeys.detail(id), 'artifacts', optionId] as const,
}
