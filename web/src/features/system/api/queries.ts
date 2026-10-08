import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { systemKeys } from './keys'

export const systemQueries = {
  /** gRPC health of every backing service plus gateway version and uptime. */
  status: () =>
    queryOptions({
      queryKey: systemKeys.status(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/system/status', { signal })),
      // Polled by the sidebar; keep it fresh enough that a page visit reuses it.
      staleTime: 10_000,
    }),

  knowledgeStats: () =>
    queryOptions({
      queryKey: systemKeys.knowledgeStats(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/knowledge/stats', { signal })),
    }),

  intelligenceStats: () =>
    queryOptions({
      queryKey: systemKeys.intelligenceStats(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/intelligence/stats', { signal })),
    }),

  deliveryStats: () =>
    queryOptions({
      queryKey: systemKeys.deliveryStats(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/delivery/stats', { signal })),
    }),
}
