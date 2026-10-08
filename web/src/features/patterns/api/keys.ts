export interface PatternFilters {
  category?: string
  includeDeprecated?: boolean
}

/** Query key factory for the pattern catalog. */
export const patternKeys = {
  all: ['patterns'] as const,
  lists: () => [...patternKeys.all, 'list'] as const,
  list: (filters: PatternFilters) => [...patternKeys.lists(), filters] as const,
  details: () => [...patternKeys.all, 'detail'] as const,
  detail: (id: string) => [...patternKeys.details(), id] as const,
}
