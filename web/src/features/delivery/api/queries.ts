import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { deliveryKeys } from './keys'

export const RUNS_LIMIT = 50
export const DEPLOYMENTS_LIMIT = 50

/** Synced snapshots: only a refresh (a mutation) changes them. */
const SYNCED_STALE_MS = 60_000

export const deliveryQueries = {
  /** Workflow definitions found in the repository. */
  pipelines: (projectId: string) =>
    queryOptions({
      queryKey: deliveryKeys.pipelines(projectId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/projects/{id}/pipelines', { params: { path: { id: projectId } }, signal })),
      staleTime: SYNCED_STALE_MS,
    }),

  /** Recent runs as last synced. `refresh=true` is a mutation, see `mutations.ts`. */
  runs: (projectId: string) =>
    queryOptions({
      queryKey: deliveryKeys.runs(projectId, RUNS_LIMIT),
      queryFn: ({ signal }) =>
        unwrap(
          api.GET('/api/v1/projects/{id}/pipeline-runs', {
            params: { path: { id: projectId }, query: { limit: RUNS_LIMIT } },
            signal,
          }),
        ),
      staleTime: SYNCED_STALE_MS,
    }),

  /** Recent deployments as last synced. */
  deployments: (projectId: string) =>
    queryOptions({
      queryKey: deliveryKeys.deployments(projectId, DEPLOYMENTS_LIMIT),
      queryFn: ({ signal }) =>
        unwrap(
          api.GET('/api/v1/projects/{id}/deployments', {
            params: { path: { id: projectId }, query: { limit: DEPLOYMENTS_LIMIT } },
            signal,
          }),
        ),
      staleTime: SYNCED_STALE_MS,
    }),
}
