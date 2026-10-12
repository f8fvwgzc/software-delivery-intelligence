import { useQuery } from '@tanstack/react-query'

import { isUserId, peopleQueries } from '@/lib/people'
import { cn } from '@/lib/utils'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Skeleton } from '@/components/ui/skeleton'

interface PersonProps {
  /** What the row stored: a user id, a plain word for automation (`system`), or nothing. */
  value?: string | null
  /** Shown when nothing is stored. */
  fallback?: string
  /** Hide the avatar where a line of text has no room for one. */
  avatar?: boolean
  className?: string
}

const WRAP = 'inline-flex max-w-full min-w-0 items-center gap-1.5 align-bottom'

/**
 * Who did something. Rows carry a user id; this names it (login and avatar)
 * through one batched lookup per render, so a table of fifty runs costs one
 * request and a person already seen costs none.
 */
export function Person({ value, fallback = '—', avatar = true, className }: PersonProps) {
  const stored = value?.trim() ?? ''
  const lookup = isUserId(stored)
  const { data: person, isPending } = useQuery({ ...peopleQueries.person(stored), enabled: lookup })

  if (!stored) return <span className={className}>{fallback}</span>
  // Automation, or an actor recorded by name: already readable.
  if (!lookup) return <span className={cn('truncate', className)}>{stored}</span>

  if (isPending) {
    return (
      <span className={cn(WRAP, className)} aria-busy="true">
        <Skeleton className="h-3 w-20" />
        <span className="sr-only">Loading who this was</span>
      </span>
    )
  }

  if (!person) {
    // Not nameable from this workspace (or the lookup failed): say so rather than print an id.
    return (
      <span className={cn('truncate', className)} title={`User ${stored}`}>
        Unknown user
      </span>
    )
  }

  const title = [person.name, person.member ? null : 'No longer a member of this workspace'].filter(Boolean).join(' · ') || undefined

  return (
    <span className={cn(WRAP, className)} title={title}>
      {avatar ? (
        <Avatar size="sm" className="size-4">
          <AvatarImage src={person.avatarUrl} alt="" />
          <AvatarFallback className="text-[8px]">{person.login.slice(0, 2).toUpperCase()}</AvatarFallback>
        </Avatar>
      ) : null}
      <span className={cn('truncate', !person.member && 'text-muted-foreground')}>{person.login}</span>
    </span>
  )
}
