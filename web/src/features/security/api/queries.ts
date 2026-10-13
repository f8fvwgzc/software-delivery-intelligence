import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { securityKeys } from './keys'

/** The server keeps its answer for five minutes; asking more often than once a minute gains nothing. */
const OVERVIEW_STALE_MS = 60_000

/**
 * `GET /projects/{id}/security`. `refresh` makes the server ask GitHub (and
 * OSV) again instead of answering from its cache; only a person's "Refresh"
 * sets it (see `useRefreshSecurity`), never the query.
 */
export function fetchSecurityOverview(projectId: string, options: { refresh?: boolean; signal?: AbortSignal } = {}) {
  return unwrap(
    api.GET('/api/v1/projects/{id}/security', {
      params: { path: { id: projectId }, query: options.refresh ? { refresh: true } : {} },
      signal: options.signal,
    }),
  )
}

export const securityQueries = {
  /**
   * The project's vulnerable dependencies and the state of Dependabot on its
   * repository, in one answer (CONTRACT.md §23.4): the page's only request.
   */
  overview: (projectId: string) =>
    queryOptions({
      queryKey: securityKeys.overview(projectId),
      queryFn: ({ signal }) => fetchSecurityOverview(projectId, { signal }),
      staleTime: OVERVIEW_STALE_MS,
    }),
}
