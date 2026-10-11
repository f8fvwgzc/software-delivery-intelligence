import { Navigate, Outlet, useLocation } from 'react-router'

import { LiveEventsProvider } from '@/app/providers/live-events-provider'
import { ErrorState } from '@/components/shared/feedback'
import { useLogout } from '@/features/auth/api/mutations'
import { useSession } from '@/features/auth/lib/auth-context'
import { loginUrl, readReturnTo } from '@/features/auth/lib/return-to'

/** Signed in, but no workspace to open: never happens per CONTRACT.md §18, and must not loop through /login if it does. */
function NoWorkspace() {
  const logout = useLogout()
  return (
    <div className="flex min-h-svh items-center justify-center bg-background p-6">
      <ErrorState
        title="No workspace to open"
        description="Your account has no personal workspace and belongs to no organisation that installed the app. Sign out and in again, or ask an organisation owner to install it."
        onRetry={() => logout.mutate()}
        retryLabel="Sign out"
        retrying={logout.isPending}
        className="w-full max-w-md"
      />
    </div>
  )
}

/**
 * Everything under the shell needs a session and a workspace. Without one
 * the user goes to the login page and comes back to the same URL after
 * GitHub. The event stream is opened here, since it is scoped to the workspace.
 */
export function RequireSession() {
  const { me, currentWorkspace } = useSession()
  const location = useLocation()

  if (!me) {
    return <Navigate to={loginUrl(`${location.pathname}${location.search}`)} replace />
  }
  if (!currentWorkspace) return <NoWorkspace />

  return (
    <LiveEventsProvider>
      <Outlet />
    </LiveEventsProvider>
  )
}

/** The login page has nothing to say to a signed-in user: on to `returnTo`. */
export function RedirectIfSignedIn() {
  const { me } = useSession()
  const location = useLocation()

  if (me) return <Navigate to={readReturnTo(location.search)} replace />
  return <Outlet />
}
