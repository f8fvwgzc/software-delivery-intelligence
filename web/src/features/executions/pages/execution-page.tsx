import { useSuspenseQuery } from '@tanstack/react-query'
import { useParams } from 'react-router'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page } from '@/components/shared/page'
import { Skeleton } from '@/components/ui/skeleton'

import { executionQueries } from '../api/queries'
import { EventLog, EventLogSkeleton } from '../components/event-log'
import { ExecutionHeader } from '../components/execution-header'
import { ExecutionRail, ExecutionRailSkeleton } from '../components/execution-rail'
import { OutputsPanel } from '../components/outputs-panel'

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

  return (
    <Page wide>
      <ExecutionHeader execution={execution} />
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,26rem)] xl:grid-cols-[minmax(0,1fr)_28rem]">
        <ExecutionRail execution={execution} />
        <aside className="flex min-w-0 flex-col gap-4 lg:sticky lg:top-16 lg:self-start">
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
    <Page wide>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <Skeleton className="h-3 w-8" />
            <Skeleton className="h-5 w-20 rounded-md" />
          </div>
          <Skeleton className="h-6 w-72 max-w-full" />
          <Skeleton className="h-3 w-96 max-w-full" />
        </div>
        <div className="flex gap-2">
          <Skeleton className="h-8 w-20 rounded-lg" />
          <Skeleton className="h-8 w-16 rounded-lg" />
        </div>
      </div>
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,26rem)] xl:grid-cols-[minmax(0,1fr)_28rem]">
        <ExecutionRailSkeleton />
        <EventLogSkeleton />
      </div>
    </Page>
  )
}
