import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Skeleton } from '@/components/ui/skeleton'
import { identityQueries } from '@/features/policies/api/queries'

/**
 * The identity an execution acts as, by name. The execution stores its id;
 * the name and what it may do live on the policies page, where this leads.
 * Reads the same short list that page uses (cached), without suspending the
 * header.
 */
export function IdentityName({ id }: { id: string }) {
  const { data: identities, isPending } = useQuery({ ...identityQueries.list(), refetchInterval: false, staleTime: 60_000 })

  if (isPending) return <Skeleton className="inline-block h-3 w-20 align-middle" />

  const identity = identities?.find((candidate) => candidate.id === id)
  if (!identity) {
    return (
      <span className="text-muted-foreground" title={`Identity ${id}`}>
        an identity that no longer exists
      </span>
    )
  }

  return (
    <Link to={routes.policies()} className="font-mono text-foreground hover:underline" title={identity.description || undefined}>
      {identity.name}
    </Link>
  )
}
