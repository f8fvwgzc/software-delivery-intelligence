import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { useEffect, useMemo, useSyncExternalStore, type ReactNode } from 'react'
import { useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { authKeys, authQueries } from '@/features/auth/api/queries'
import { AuthContext, type AuthContextValue } from '@/features/auth/lib/auth-context'
import { onUnauthenticated } from '@/lib/api/session-events'
import { workspaceStore } from '@/lib/workspace-store'

/**
 * Loads `/auth/me` once (suspending the whole tree until it answers) and
 * exposes the session. Lives inside the router so a workspace switch can
 * navigate; it must be wrapped in a Suspense boundary (see RootLayout).
 */
export function AuthProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const { data: me } = useSuspenseQuery(authQueries.me())
  const workspaceId = useSyncExternalStore(workspaceStore.subscribe, workspaceStore.get)

  // A 401 from any endpoint: the session is gone, drop the user and let the guard redirect.
  useEffect(
    () =>
      onUnauthenticated(() => {
        queryClient.setQueryData(authKeys.me(), null)
      }),
    [queryClient],
  )

  const value = useMemo<AuthContextValue>(() => {
    const currentWorkspace = me?.workspaces.find((workspace) => workspace.id === workspaceId) ?? null
    return {
      me,
      currentWorkspace,
      setWorkspace: (id) => {
        if (id === workspaceStore.get()) return
        workspaceStore.set(id)
        // Every entity on screen belonged to the old workspace: start over on
        // the overview, with an empty cache. The event stream reopens on its
        // own because its URL carries the workspace.
        navigate(routes.overview())
        queryClient.resetQueries({ predicate: (query) => query.queryKey[0] !== authKeys.all[0] })
      },
    }
  }, [me, workspaceId, navigate, queryClient])

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}
