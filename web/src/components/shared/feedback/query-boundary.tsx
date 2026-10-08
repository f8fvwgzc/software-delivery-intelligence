import { QueryErrorResetBoundary } from '@tanstack/react-query'
import { Suspense, type ReactNode } from 'react'
import { ErrorBoundary, type FallbackProps } from 'react-error-boundary'

import { ErrorState } from './error-state'
import { PageSkeleton } from './page-skeleton'

interface QueryBoundaryProps {
  children: ReactNode
  /** Shown while a lazy chunk or a suspense query is loading. */
  fallback?: ReactNode
}

function Fallback({ error, resetErrorBoundary }: FallbackProps) {
  return <ErrorState error={error} onRetry={resetErrorBoundary} />
}

/**
 * Suspense + error boundary pair for a page or a panel. Errors from
 * `useSuspenseQuery` are reset on retry so the query runs again.
 */
export function QueryBoundary({ children, fallback }: QueryBoundaryProps) {
  return (
    <QueryErrorResetBoundary>
      {({ reset }) => (
        <ErrorBoundary onReset={reset} FallbackComponent={Fallback}>
          <Suspense fallback={fallback ?? <PageSkeleton />}>{children}</Suspense>
        </ErrorBoundary>
      )}
    </QueryErrorResetBoundary>
  )
}
