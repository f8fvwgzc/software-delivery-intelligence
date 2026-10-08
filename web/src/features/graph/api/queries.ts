import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { graphKeys } from './keys'

export const graphQueries = {
  /** Nodes and edges of a project; `includeHistory` also returns edges that stopped holding. */
  graph: (projectId: string, includeHistory: boolean) =>
    queryOptions({
      queryKey: graphKeys.graph(projectId, includeHistory),
      queryFn: ({ signal }) =>
        unwrap(
          api.GET('/api/v1/projects/{id}/graph', {
            params: { path: { id: projectId }, query: { includeHistory } },
            signal,
          }),
        ),
      staleTime: 60_000,
    }),

  /**
   * What a change to one node would touch. A POST by contract, but it is a pure
   * read, so it is cached per node like any other query.
   */
  impact: (projectId: string, nodeId: string) =>
    queryOptions({
      queryKey: graphKeys.impact(projectId, nodeId),
      queryFn: ({ signal }) =>
        unwrap(
          api.POST('/api/v1/projects/{id}/impact', {
            params: { path: { id: projectId } },
            body: { nodeId },
            signal,
          }),
        ),
      staleTime: 5 * 60_000,
    }),
}
