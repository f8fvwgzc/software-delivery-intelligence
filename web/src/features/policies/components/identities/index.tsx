import { useSuspenseQuery } from '@tanstack/react-query'
import { FingerprintIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Skeleton } from '@/components/ui/skeleton'

import { identityQueries } from '../../api/queries'
import { IdentityCard } from './identity-card'

/** Every identity executions can run as, with its live grants. */
export function Identities() {
  const { data: identities } = useSuspenseQuery(identityQueries.list())

  if (identities.length === 0) {
    return (
      <EmptyState
        icon={FingerprintIcon}
        title="No identities"
        description="The execution service seeds the simulated identity on start."
      />
    )
  }

  return (
    <div className="grid gap-4 md:grid-cols-2">
      {identities.map((identity) => (
        <IdentityCard key={identity.id} identity={identity} />
      ))}
    </div>
  )
}

export function IdentitiesSkeleton({ cards = 2 }: { cards?: number }) {
  return (
    <div className="grid gap-4 md:grid-cols-2">
      {Array.from({ length: cards }, (_, index) => (
        <div key={index} className="flex flex-col gap-3 rounded-lg p-3 ring-1 ring-border">
          <div className="flex items-start justify-between gap-3">
            <div className="flex flex-col gap-1.5">
              <Skeleton className="h-4 w-32" />
              <Skeleton className="h-3 w-56" />
            </div>
            <Skeleton className="h-7 w-32 rounded-md" />
          </div>
          <div className="flex flex-col gap-2">
            <Skeleton className="h-3 w-full" />
            <Skeleton className="h-3 w-5/6" />
            <Skeleton className="h-3 w-2/3" />
          </div>
        </div>
      ))}
    </div>
  )
}
