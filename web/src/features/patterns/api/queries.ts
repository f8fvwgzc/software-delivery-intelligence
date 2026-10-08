import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { patternKeys, type PatternFilters } from './keys'

export const patternsQuery = (filters: PatternFilters = {}) =>
  queryOptions({
    queryKey: patternKeys.list(filters),
    queryFn: () =>
      unwrap(
        api.GET('/api/v1/patterns', {
          params: {
            query: {
              ...(filters.category ? { category: filters.category } : {}),
              ...(filters.includeDeprecated ? { includeDeprecated: true } : {}),
            },
          },
        }),
      ),
    staleTime: 5 * 60_000,
  })

export const patternQuery = (patternId: string) =>
  queryOptions({
    queryKey: patternKeys.detail(patternId),
    queryFn: () => unwrap(api.GET('/api/v1/patterns/{id}', { params: { path: { id: patternId } } })),
  })
