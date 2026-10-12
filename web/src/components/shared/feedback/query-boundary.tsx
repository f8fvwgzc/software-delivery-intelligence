import { QueryErrorResetBoundary } from '@tanstack/react-query'
import { Suspense, useEffect, type ReactNode } from 'react'
import { ErrorBoundary, type FallbackProps } from 'react-error-boundary'

import { isNetworkError } from '@/lib/api/errors'

import { ErrorState } from './error-state'
import { PageSkeleton } from './page-skeleton'

interface QueryBoundaryProps {
  children: ReactNode
  /** Shown while a lazy chunk or a suspense query is loading. */
  fallback?: ReactNode
  /** Inside a frame: the error renders small, without its own box. */
  compact?: boolean
}

/**
 * A failed fetch is retried by itself the moment the browser reports the
 * connection is back; every other error waits for the retry button.
 */
function useRetryWhenOnline(error: unknown, retry: () => void) {
  const network = isNetworkError(error)
  useEffect(() => {
    if (!network) return
    window.addEventListener('online', retry)
    return () => window.removeEventListener('online', retry)
  }, [network, retry])
}

function Fallback({ error, resetErrorBoundary }: FallbackProps) {
  useRetryWhenOnline(error, resetErrorBoundary)
  return <ErrorState error={error} onRetry={resetErrorBoundary} />
}

function CompactFallback({ error, resetErrorBoundary }: FallbackProps) {
  useRetryWhenOnline(error, resetErrorBoundary)
  return <ErrorState error={error} onRetry={resetErrorBoundary} compact />
}

/**
 * Suspense + error boundary pair for a page or a panel. Errors from
 * `useSuspenseQuery` are reset on retry so the query runs again.
 */
export function QueryBoundary({ children, fallback, compact = false }: QueryBoundaryProps) {
  return (
    <QueryErrorResetBoundary>
      {({ reset }) => (
        <ErrorBoundary onReset={reset} FallbackComponent={compact ? CompactFallback : Fallback}>
          <Suspense fallback={fallback ?? <PageSkeleton />}>{children}</Suspense>
        </ErrorBoundary>
      )}
    </QueryErrorResetBoundary>
  )
}
