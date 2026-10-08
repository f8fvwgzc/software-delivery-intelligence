export interface DecisionFilters {
  projectId?: string
  includeSuperseded?: boolean
}

/** Query key factory for decisions. The root is also invalidated by proposals. */
export const decisionKeys = {
  all: ['decisions'] as const,
  lists: () => [...decisionKeys.all, 'list'] as const,
  list: (filters: DecisionFilters) => [...decisionKeys.lists(), filters] as const,
}
