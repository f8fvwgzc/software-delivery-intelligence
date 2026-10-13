import { useMutation, useQueryClient } from '@tanstack/react-query'

import { approvalKeys } from '@/features/approvals/api/keys'
import { executionKeys } from '@/features/executions/api/keys'
import { api, unwrap } from '@/lib/api/client'

import { securityKeys } from './keys'
import { fetchSecurityOverview } from './queries'
import type { ConfigureDependabotRequest, SecurityOverview } from './types'

/**
 * `GET /projects/{id}/security?refresh=true`: makes the server ask GitHub
 * (and OSV) again instead of answering from its five-minute cache. A
 * mutation, not a refetch: it is a person's request with its own pending
 * state, and the query itself must keep asking for the cached answer.
 */
export function useRefreshSecurity(projectId: string) {
  const queryClient = useQueryClient()
  const key = securityKeys.overview(projectId)
  return useMutation({
    mutationFn: async () => {
      // An answer from the cache that is still on its way must not land on top of the fresh one.
      await queryClient.cancelQueries({ queryKey: key })
      return fetchSecurityOverview(projectId, { refresh: true })
    },
    onSuccess: (overview) => queryClient.setQueryData(key, overview),
  })
}

/**
 * `POST /projects/{id}/security/dependabot` (admin): one execution that
 * turns on alerts, security updates and/or opens the pull request that adds
 * `.github/dependabot.yml`. The dialog shows a refusal itself, so the
 * mutation opts out of the global error toast (`lib/query-client.ts`).
 */
export function useConfigureDependabot(projectId: string) {
  const queryClient = useQueryClient()
  const key = securityKeys.overview(projectId)
  return useMutation({
    mutationFn: (body: ConfigureDependabotRequest) =>
      unwrap(api.POST('/api/v1/projects/{id}/security/dependabot', { params: { path: { id: projectId } }, body })),
    meta: { silent: true },
    onSuccess: ({ executionId }) => {
      // The server just named the execution: the page links to it instead of offering the button again.
      queryClient.setQueryData<SecurityOverview>(key, (overview) => (overview ? { ...overview, configureExecutionId: executionId } : overview))
      queryClient.invalidateQueries({ queryKey: executionKeys.lists() })
      queryClient.invalidateQueries({ queryKey: approvalKeys.all })
    },
  })
}
