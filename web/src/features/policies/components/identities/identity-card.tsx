import { RelativeTime } from '@/components/shared/data'
import { Badge } from '@/components/ui/badge'
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { IDENTITY_KIND, isExpired } from '../../lib/labels'
import { GrantDialog } from './grant-dialog'

interface IdentityCardProps {
  identity: components['schemas']['Identity']
}

export function IdentityCard({ identity }: IdentityCardProps) {
  const grants = [...identity.grants].sort((a, b) => {
    // Base scope first, then soonest expiry.
    if (!a.expiresAt) return -1
    if (!b.expiresAt) return 1
    return a.expiresAt.localeCompare(b.expiresAt)
  })

  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          {identity.name}
          <Badge variant="outline" className="h-5 font-normal text-muted-foreground">
            {IDENTITY_KIND[identity.kind] ?? identity.kind}
          </Badge>
        </CardTitle>
        <CardDescription>{identity.description}</CardDescription>
        <CardAction>
          <GrantDialog identity={identity} />
        </CardAction>
      </CardHeader>
      <CardContent>
        {grants.length === 0 ? (
          <p className="text-xs text-muted-foreground">No permissions. Every step will stop at the permission check.</p>
        ) : (
          <ul className="flex flex-col divide-y" aria-label={`Grants held by ${identity.name}`}>
            {grants.map((grant) => {
              const expired = isExpired(grant.expiresAt)
              return (
                <li
                  key={`${grant.permission}-${grant.expiresAt ?? 'base'}`}
                  className={cn('flex items-center justify-between gap-3 py-1.5 text-xs', expired && 'text-muted-foreground line-through')}
                >
                  <span className={cn('font-mono', expired && 'line-through decoration-muted-foreground/40')}>{grant.permission}</span>
                  <span className="shrink-0 text-muted-foreground">
                    {grant.expiresAt === null || grant.expiresAt === undefined ? (
                      'base scope'
                    ) : expired ? (
                      <>
                        expired <RelativeTime value={grant.expiresAt} className="font-mono tabular-nums" />
                      </>
                    ) : (
                      <>
                        expires <RelativeTime value={grant.expiresAt} className="font-mono tabular-nums text-foreground/80" />
                      </>
                    )}
                  </span>
                </li>
              )
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  )
}
