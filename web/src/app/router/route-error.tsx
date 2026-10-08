import { isRouteErrorResponse, useRouteError } from 'react-router'

import { ErrorState } from '@/components/shared/feedback/error-state'

/** Last line of defence: shown when the shell itself fails to render. */
export function RouteError() {
  const error = useRouteError()
  const message = isRouteErrorResponse(error)
    ? `${error.status} ${error.statusText}`
    : error instanceof Error
      ? error.message
      : 'Unexpected error'

  return (
    <div className="flex min-h-svh items-center justify-center p-6">
      <ErrorState
        title="The app hit an unexpected error"
        description={message}
        onRetry={() => window.location.reload()}
        retryLabel="Reload"
      />
    </div>
  )
}
