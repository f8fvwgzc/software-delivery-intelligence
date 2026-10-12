import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { searchKeys, type SearchFilters } from './keys'

export const SEARCH_LIMIT = 25

export const searchQuery = (filters: SearchFilters) =>
  queryOptions({
    queryKey: searchKeys.results(filters),
    queryFn: ({ signal }) =>
      unwrap(
        api.GET('/api/v1/knowledge/search', {
          params: {
            query: {
              q: filters.q,
              limit: SEARCH_LIMIT,
              ...(filters.projectId ? { projectId: filters.projectId } : {}),
              ...(filters.kinds.length > 0 ? { kinds: filters.kinds.join(',') } : {}),
            },
          },
          signal,
        }),
      ),
    staleTime: 60_000,
  })
