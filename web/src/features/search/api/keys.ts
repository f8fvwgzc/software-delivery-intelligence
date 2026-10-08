export interface SearchFilters {
  q: string
  projectId?: string
  /** Sorted, so the same selection always yields the same key. */
  kinds: string[]
}

/** Query key factory for semantic knowledge search. */
export const searchKeys = {
  all: ['knowledge-search'] as const,
  results: (filters: SearchFilters) => [...searchKeys.all, filters] as const,
}
