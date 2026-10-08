import { useMutation, useQueryClient } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { deliveryKeys } from './keys'
import { DEPLOYMENTS_LIMIT, RUNS_LIMIT } from './queries'

/**
 * Re-sync runs from the provider. The endpoint is a GET by contract, but
 * `refresh=true` makes it an action, so it is modelled as a mutation: the
 * button shows its loader and the whole delivery family is invalidated after.
 */
export function useRefreshPipelineRuns(projectId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: () =>
      unwrap(
        api.GET('/api/v1/projects/{id}/pipeline-runs', {
          params: { path: { id: projectId }, query: { refresh: true, limit: RUNS_LIMIT } },
        }),
      ),
    onSuccess: (data) => {
      queryClient.setQueryData(deliveryKeys.runs(projectId, RUNS_LIMIT), data)
      return queryClient.invalidateQueries({ queryKey: deliveryKeys.project(projectId) })
    },
  })
}

/** Same for deployments. */
export function useRefreshDeployments(projectId: string) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: () =>
      unwrap(
        api.GET('/api/v1/projects/{id}/deployments', {
          params: { path: { id: projectId }, query: { refresh: true, limit: DEPLOYMENTS_LIMIT } },
        }),
      ),
    onSuccess: (data) => {
      queryClient.setQueryData(deliveryKeys.deployments(projectId, DEPLOYMENTS_LIMIT), data)
      return queryClient.invalidateQueries({ queryKey: deliveryKeys.project(projectId) })
    },
  })
}
