import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { identityKeys, policyKeys } from './keys'

export const policyQueries = {
  list: () =>
    queryOptions({
      queryKey: policyKeys.list(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/policies', { signal })),
      // Only this page changes them, through mutations that invalidate the list.
      staleTime: 60_000,
    }),
}

export const identityQueries = {
  list: () =>
    queryOptions({
      queryKey: identityKeys.list(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/identities', { signal })),
      // Grants expire on a clock; a periodic refetch keeps the expiry display honest.
      refetchInterval: 60_000,
    }),
}
