import { useQuery } from '@tanstack/react-query'
import { ExternalLinkIcon } from 'lucide-react'

import { routes } from '@/app/router/routes'
import { ApiError } from '@/lib/api/errors'

import { authQueries } from '../../api/queries'
import { gatewayLoginUrl } from '../../lib/return-to'

const LINK_CLASS = 'inline-flex items-center gap-1 text-foreground underline-offset-4 hover:underline'

/**
 * The organisation path. The install URL needs a session, so while signed
 * out the line sends the user through sign-in first and lands on the
 * connect page, where the install action lives too.
 */
export function InstallLine() {
  const installUrl = useQuery(authQueries.installUrl())

  if (installUrl.isError && installUrl.error instanceof ApiError && !installUrl.error.isUnauthenticated) {
    // Typically `failed_precondition`: the GitHub App is not configured on this deployment.
    return <p className="text-xs text-muted-foreground">Organisations: the GitHub App is not configured on this deployment.</p>
  }

  const href = installUrl.data?.url ?? gatewayLoginUrl(routes.connect())

  return (
    <p className="text-xs text-muted-foreground">
      Organisations:{' '}
      <a href={href} className={LINK_CLASS}>
        install the GitHub App
        <ExternalLinkIcon className="size-3" aria-hidden />
      </a>
      {installUrl.data ? null : ' after signing in.'}
    </p>
  )
}
