import { QueryErrorResetBoundary } from '@tanstack/react-query'
import { Suspense } from 'react'
import { ErrorBoundary, type FallbackProps } from 'react-error-boundary'
import { Outlet } from 'react-router'

import { AuthProvider } from '@/app/providers/auth-provider'
import { ErrorState } from '@/components/shared/feedback'
import { Spinner } from '@/components/ui/spinner'

/** Full-viewport wait while `/auth/me` answers or the first route chunk loads. */
function SessionFallback() {
  return (
    <div className="flex min-h-svh items-center justify-center bg-background" role="status" aria-label="Loading">
      <Spinner className="size-5 text-muted-foreground" />
    </div>
  )
}

function SessionError({ error, resetErrorBoundary }: FallbackProps) {
  return (
    <div className="flex min-h-svh items-center justify-center bg-background p-6">
      <ErrorState
        error={error}
        title="Could not reach the gateway"
        onRetry={resetErrorBoundary}
        className="w-full max-w-md"
      />
    </div>
  )
}

/**
 * Outermost route element: resolves the session before anything else
 * renders. Both the login page and the authenticated shell sit below it.
 */
export function RootLayout() {
  return (
    <QueryErrorResetBoundary>
      {({ reset }) => (
        <ErrorBoundary onReset={reset} FallbackComponent={SessionError}>
          <Suspense fallback={<SessionFallback />}>
            <AuthProvider>
              <Outlet />
            </AuthProvider>
          </Suspense>
        </ErrorBoundary>
      )}
    </QueryErrorResetBoundary>
  )
}
