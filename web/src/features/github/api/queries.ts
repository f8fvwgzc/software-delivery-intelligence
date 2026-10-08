import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { githubKeys } from './keys'

export const githubQueries = {
  /** The account behind the configured token, or `tokenConfigured: false`. */
  account: () =>
    queryOptions({
      queryKey: githubKeys.account(),
      queryFn: ({ signal }) => unwrap(api.GET('/api/v1/github/account', { signal })),
      // A token rarely changes while the app is open.
      staleTime: 5 * 60_000,
    }),

  /** Repositories the token can see, filtered server-side by name. */
  repos: (query: string) =>
    queryOptions({
      queryKey: githubKeys.repoList(query),
      queryFn: ({ signal }) =>
        unwrap(
          api.GET('/api/v1/github/repos', {
            params: { query: query ? { query } : {} },
            signal,
          }),
        ),
      staleTime: 60_000,
    }),
}
