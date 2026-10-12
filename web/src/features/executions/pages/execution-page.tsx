import { useSuspenseQuery } from '@tanstack/react-query'
import { useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Skeleton } from '@/components/ui/skeleton'

import { executionQueries } from '../api/queries'
import { EventLog, EventLogSkeleton } from '../components/event-log'
import { ExecutionHeader } from '../components/execution-header'
import { ExecutionRail, ExecutionRailSkeleton } from '../components/execution-rail'
import { OutputsPanel } from '../components/outputs-panel'
import { stepProgress } from '../lib/progress'

const GRID_CLASS = 'grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,26rem)] xl:grid-cols-[minmax(0,1fr)_28rem]'

export default function ExecutionPage() {
  const { executionId = '' } = useParams()

  return (
    <QueryBoundary fallback={<ExecutionPageSkeleton />}>
      <ExecutionView key={executionId} executionId={executionId} />
    </QueryBoundary>
  )
}

function ExecutionView({ executionId }: { executionId: string }) {
  const { data: execution } = useSuspenseQuery(executionQueries.detail(executionId))
  const progress = stepProgress(execution.steps)

  return (
    <Page>
      <ExecutionHeader execution={execution} />
      <div className={GRID_CLASS}>
        <Frame
          title="Steps"
          description="In plan order, one checkpoint after each."
          meta={
            <span className="text-2xs tabular-nums text-muted-foreground">
              {progress.succeeded}/{progress.total} done
            </span>
          }
        >
          <ExecutionRail execution={execution} />
        </Frame>
        <aside className="flex min-w-0 flex-col gap-4 lg:sticky lg:top-[calc(var(--header-h)+1rem)] lg:self-start">
          <OutputsPanel outputs={execution.outputs} />
          <QueryBoundary fallback={<EventLogSkeleton />}>
            <EventLog executionId={execution.id} status={execution.status} steps={execution.steps} />
          </QueryBoundary>
        </aside>
      </div>
    </Page>
  )
}

function ExecutionPageSkeleton() {
  return (
    <Page>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <Skeleton className="h-3 w-8" />
            <Skeleton className="h-5 w-20 rounded-[4px]" />
          </div>
          <Skeleton className="h-5 w-72 max-w-full" />
          <Skeleton className="h-3 w-96 max-w-full" />
        </div>
        <div className="flex gap-2">
          <Skeleton className="h-8 w-20 rounded-lg" />
          <Skeleton className="h-8 w-16 rounded-lg" />
        </div>
      </div>
      <div className={GRID_CLASS}>
        <Frame title="Steps">
          <ExecutionRailSkeleton />
        </Frame>
        <EventLogSkeleton />
      </div>
    </Page>
  )
}
