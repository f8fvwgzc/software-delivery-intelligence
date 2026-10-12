import type { UseQueryResult } from '@tanstack/react-query'
import { RadarIcon } from 'lucide-react'
import { useMemo } from 'react'

import { EmptyState, ErrorState } from '@/components/shared/feedback'
import { Skeleton } from '@/components/ui/skeleton'

import type { ImpactAnalysis, ImpactedNode } from '../../lib/types'
import { ImpactGroup } from './impact-group'
import { count } from '@/lib/intl'

interface ImpactAnalysisSectionProps {
  projectId: string
  impact: UseQueryResult<ImpactAnalysis>
  onSelectNode: (id: string) => void
}

/** Rows shaped like the impact list, so the panel does not jump when results land. */
function ImpactSkeleton() {
  return (
    <div className="flex flex-col gap-2" aria-busy aria-label="Analysing impact">
      <Skeleton className="h-3.5 w-3/4" />
      <div className="mt-1 flex flex-col gap-1">
        <Skeleton className="mx-2 h-3 w-10" />
        {Array.from({ length: 3 }, (_, index) => (
          <div key={index} className="flex items-start gap-2 px-2 py-1.5">
            <Skeleton className="size-5 rounded-sm" />
            <div className="flex flex-1 flex-col gap-1">
              <Skeleton className="h-3.5 w-2/3" />
              <Skeleton className="h-3 w-1/2" />
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

function groupByDistance(impacted: ImpactedNode[]) {
  const groups = new Map<number, ImpactedNode[]>()
  for (const item of impacted) {
    const list = groups.get(item.distance)
    if (list) list.push(item)
    else groups.set(item.distance, [item])
  }
  return [...groups.entries()].sort(([a], [b]) => a - b)
}

/** What a change to the selected node would touch, nearest first. */
export function ImpactAnalysisSection({ projectId, impact, onSelectNode }: ImpactAnalysisSectionProps) {
  const groups = useMemo(() => (impact.data ? groupByDistance(impact.data.impacted) : []), [impact.data])

  return (
    <section className="flex flex-col gap-2" aria-labelledby="impact-heading">
      <div className="flex items-baseline justify-between gap-2">
        <h3 id="impact-heading" className="text-xs font-medium">
          Impact analysis
        </h3>
        {impact.data ? (
          <span className="text-2xs tabular-nums text-muted-foreground">
            {count(impact.data.impacted.length, 'node', 'nodes')}
          </span>
        ) : null}
      </div>

      {impact.isPending ? (
        <ImpactSkeleton />
      ) : impact.isError ? (
        <ErrorState
          error={impact.error}
          title="Impact analysis failed"
          onRetry={() => void impact.refetch()}
          retrying={impact.isFetching}
          className="py-6"
        />
      ) : impact.data.impacted.length === 0 ? (
        <EmptyState
          icon={RadarIcon}
          title="Nothing reachable"
          description="No other node is connected to this one. A change here stays local."
          className="py-6"
        />
      ) : (
        <div className="flex flex-col gap-3">
          {impact.data.summary ? <p className="text-xs text-muted-foreground">{impact.data.summary}</p> : null}
          <div className="-mx-2 flex flex-col gap-3">
            {groups.map(([distance, items]) => (
              <ImpactGroup key={distance} projectId={projectId} distance={distance} items={items} onSelect={onSelectNode} />
            ))}
          </div>
        </div>
      )}
    </section>
  )
}
