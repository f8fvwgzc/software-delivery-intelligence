/** Query key factory for proposals and their negotiation threads. */
export const proposalKeys = {
  all: ['proposals'] as const,
  lists: () => [...proposalKeys.all, 'list'] as const,
  list: (filters: { projectId?: string }) => [...proposalKeys.lists(), filters] as const,
  details: () => [...proposalKeys.all, 'detail'] as const,
  detail: (id: string) => [...proposalKeys.details(), id] as const,
  messages: (id: string) => [...proposalKeys.detail(id), 'messages'] as const,
}

/**
 * Projects are owned by the projects feature; this key mirrors its family so
 * an invalidation there refreshes the pickers here too.
 */
export const projectPickerKeys = {
  all: ['projects'] as const,
  list: () => [...projectPickerKeys.all, 'list'] as const,
}
