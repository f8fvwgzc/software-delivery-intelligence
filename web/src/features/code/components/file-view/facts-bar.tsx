import { useQuery } from '@tanstack/react-query'
import { SparklesIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Badge } from '@/components/ui/badge'
import { Skeleton } from '@/components/ui/skeleton'
import { understandingQueries } from '@/features/understanding/api/queries'

interface FactsBarProps {
  projectId: string
  factIds: string[]
}

/** The facts the scan read from this file, each a link back to the Understanding page. */
export function FactsBar({ projectId, factIds }: FactsBarProps) {
  const { data: facts, isPending } = useQuery(understandingQueries.facts(projectId, 'detected'))

  const wanted = new Set(factIds)
  const matched = facts?.filter((fact) => wanted.has(fact.id)) ?? []
  const unlisted = factIds.length - matched.length

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 border-b bg-muted/30 px-3 py-2">
      <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
        <SparklesIcon className="size-3.5" aria-hidden="true" />
        Facts detected from this file
      </span>
      <ul className="flex flex-wrap items-center gap-1.5" aria-label="Detected facts">
        {isPending
          ? factIds.map((id) => (
              <li key={id}>
                <Skeleton className="h-5 w-28 rounded-md" />
              </li>
            ))
          : matched.map((fact) => (
              <li key={fact.id}>
                <Badge variant="outline" asChild>
                  <Link to={routes.project(projectId)} title={fact.evidence || fact.key}>
                    <span className="truncate">{fact.label}</span>
                    {fact.value ? <span className="font-mono text-muted-foreground">{fact.value}</span> : null}
                  </Link>
                </Badge>
              </li>
            ))}
        {!isPending && unlisted > 0 ? (
          <li className="text-2xs tabular-nums text-muted-foreground">
            {matched.length > 0 ? `+${unlisted} more` : `${unlisted} ${unlisted === 1 ? 'fact' : 'facts'}`}
          </li>
        ) : null}
      </ul>
    </div>
  )
}
