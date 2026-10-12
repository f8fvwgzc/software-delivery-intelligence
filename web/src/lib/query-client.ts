import { MutationCache, QueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import { ApiError, isNetworkError } from '@/lib/api/errors'

/**
 * Client errors will not fix themselves; only retry what might. A failed
 * fetch while the browser knows it is offline is not retried either: the
 * panel shows its offline copy at once and refetches when the connection
 * is back (see QueryBoundary).
 */
function shouldRetry(failureCount: number, error: unknown) {
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) {
    return false
  }
  if (isNetworkError(error) && typeof navigator !== 'undefined' && !navigator.onLine) {
    return false
  }
  return failureCount < 2
}

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      /**
       * Everything scoped to a commit (tree, files, tokens, graph, …) sets
       * its own, longer staleTime; live entities (projects, executions,
       * approvals, reviews) sit at 10 s and are kept fresh by the platform
       * event stream in between (CONTRACT.md §21).
       */
      staleTime: 10_000,
      gcTime: 5 * 60_000,
      refetchOnWindowFocus: false,
      retry: shouldRetry,
      /**
       * `always`: an offline browser fails fast and renders an error state
       * with a retry, instead of suspending on a paused query with nothing
       * to click (the retry resets the boundary; the panel also refetches
       * on its own when the connection returns).
       */
      networkMode: 'always',
    },
    mutations: {
      retry: false,
      networkMode: 'always',
    },
  },
  // One place for mutation failures, so no button ever fails silently.
  // A mutation can opt out with `meta: { silent: true }`.
  mutationCache: new MutationCache({
    onError: (error, _variables, _context, mutation) => {
      if (mutation.meta?.silent) return
      const title = error instanceof ApiError ? error.title : isNetworkError(error) ? 'Could not reach the server' : 'Something went wrong'
      const description = isNetworkError(error)
        ? typeof navigator !== 'undefined' && !navigator.onLine
          ? 'You are offline. Nothing was changed; try again once the connection is back.'
          : 'The gateway did not answer. Nothing was changed; try again in a moment.'
        : error.message
      toast.error(title, { description })
    },
  }),
})
