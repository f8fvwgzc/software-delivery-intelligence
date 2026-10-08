import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { identityKeys, policyKeys } from './keys'

export const policyQueries = {
  list: () =>
    queryOptions({
      queryKey: policyKeys.list(),
      queryFn: () => unwrap(api.GET('/api/v1/policies')),
    }),
}

export const identityQueries = {
  list: () =>
    queryOptions({
      queryKey: identityKeys.list(),
      queryFn: () => unwrap(api.GET('/api/v1/identities')),
      // Grants expire on a clock; a periodic refetch keeps the expiry display honest.
      refetchInterval: 60_000,
    }),
}
