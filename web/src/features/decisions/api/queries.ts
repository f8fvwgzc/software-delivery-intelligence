import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { decisionKeys, type DecisionFilters } from './keys'

export const decisionsQuery = (filters: DecisionFilters = {}) =>
  queryOptions({
    queryKey: decisionKeys.list(filters),
    queryFn: () =>
      unwrap(
        api.GET('/api/v1/decisions', {
          params: {
            query: {
              ...(filters.projectId ? { projectId: filters.projectId } : {}),
              ...(filters.includeSuperseded ? { includeSuperseded: true } : {}),
            },
          },
        }),
      ),
  })
