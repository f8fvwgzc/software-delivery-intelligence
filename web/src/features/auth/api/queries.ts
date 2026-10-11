import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import { ApiError } from '@/lib/api/errors'
import { workspaceStore } from '@/lib/workspace-store'

import { authKeys } from './keys'
import type { InstallUrl, Me } from './types'

export { authKeys }

/**
 * The workspace this tab works in: the remembered one when the user can
 * still open it, else the one the gateway resolved the request in. Settled
 * here, before anything renders, so the first page query already carries
 * the right header.
 */
function reconcileWorkspace(me: Me) {
  const remembered = workspaceStore.get()
  const stillOpen = remembered && me.workspaces.some((workspace) => workspace.id === remembered)
  if (!stillOpen) workspaceStore.set(me.currentWorkspaceId)
}

export const authQueries = {
  /** `GET /auth/me`: the session, or `null` while signed out (a 401 is a state, not a failure). */
  me: () =>
    queryOptions({
      queryKey: authKeys.me(),
      queryFn: async ({ signal }): Promise<Me | null> => {
        try {
          const me = await unwrap(api.GET('/api/v1/auth/me', { signal }))
          reconcileWorkspace(me)
          return me
        } catch (error) {
          if (error instanceof ApiError && error.isUnauthenticated) return null
          throw error
        }
      },
      // Loaded once per tab; a 401 anywhere else resets it (see the auth provider).
      staleTime: Infinity,
      gcTime: Infinity,
      retry: false,
    }),

  /** Where an organisation owner installs the SDI GitHub App; fails while the app is not configured. */
  installUrl: () =>
    queryOptions({
      queryKey: authKeys.installUrl(),
      queryFn: ({ signal }): Promise<InstallUrl> => unwrap(api.GET('/api/v1/github/app/install-url', { signal })),
      staleTime: 5 * 60_000,
      retry: false,
    }),
}
