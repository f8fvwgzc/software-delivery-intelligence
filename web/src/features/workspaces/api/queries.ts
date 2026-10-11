import { infiniteQueryOptions, queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'

import { workspaceKeys } from './keys'
import type { AuditEntry } from './types'

export { workspaceKeys }

/** Entries per audit page; a full page means there may be an earlier one. */
export const AUDIT_PAGE_SIZE = 50

export const workspaceQueries = {
  /** Members as last synced from GitHub, with derived and effective roles. */
  members: (workspaceId: string) =>
    queryOptions({
      queryKey: workspaceKeys.members(workspaceId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/workspaces/{id}/members', { params: { path: { id: workspaceId } }, signal })),
      staleTime: 60_000,
    }),

  /** Newest first; each page ends where the next one starts (`before=` the last entry's time). */
  audit: (workspaceId: string, actionPrefix: string) =>
    infiniteQueryOptions({
      queryKey: workspaceKeys.audit(workspaceId, actionPrefix),
      queryFn: ({ pageParam, signal }): Promise<AuditEntry[]> =>
        unwrap(
          api.GET('/api/v1/workspaces/{id}/audit', {
            params: {
              path: { id: workspaceId },
              query: {
                limit: AUDIT_PAGE_SIZE,
                ...(actionPrefix ? { actionPrefix } : {}),
                ...(pageParam ? { before: pageParam } : {}),
              },
            },
            signal,
          }),
        ),
      initialPageParam: undefined as string | undefined,
      getNextPageParam: (lastPage) => (lastPage.length === AUDIT_PAGE_SIZE ? lastPage.at(-1)?.at : undefined),
      staleTime: 30_000,
    }),

  /** The §19 controls: code cache, chunks, reasoner, audit retention. */
  dataControls: (workspaceId: string) =>
    queryOptions({
      queryKey: workspaceKeys.dataControls(workspaceId),
      queryFn: ({ signal }) =>
        unwrap(api.GET('/api/v1/workspaces/{id}/data-controls', { params: { path: { id: workspaceId } }, signal })),
      staleTime: 60_000,
    }),
}
