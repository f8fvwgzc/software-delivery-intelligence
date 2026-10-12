import { useSuspenseInfiniteQuery } from '@tanstack/react-query'
import { ActivityIcon, FilterXIcon } from 'lucide-react'
import { useState, type ReactNode } from 'react'

import { EmptyState } from '@/components/shared/feedback'
import { FrameFooter } from '@/components/system/frame'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'
import { count } from '@/lib/intl'

import { executionQueries } from '../../api/queries'
import { adapterLabel } from '../../lib/labels'
import { AdapterFilterGroup, type AdapterFilter } from './adapter-filter'
import { ExecutionRow } from './row'

export { ExecutionsTableSkeleton } from './skeleton'

type Execution = components['schemas']['Execution']

interface ExecutionsTableProps {
  /** Scope the list to one project; the project column is hidden then. */
  projectId?: string
  /** Primary action for the empty state, e.g. the "New dry run" trigger. */
  emptyAction?: ReactNode
}

function countByAdapter(executions: Execution[]) {
  const counts: Partial<Record<AdapterFilter, number>> = { all: executions.length }
  for (const execution of executions) {
    const key = execution.adapter as AdapterFilter
    counts[key] = (counts[key] ?? 0) + 1
  }
  return counts
}

/**
 * Executions newest first, one page at a time with "Load earlier" at the
 * foot; live while any is non-terminal. The adapter filter is client-side.
 */
export function ExecutionsTable({ projectId, emptyAction }: ExecutionsTableProps) {
  const query = useSuspenseInfiniteQuery(executionQueries.pages(projectId ? { projectId } : {}))
  const executions = query.data.pages.flat()
  const [adapter, setAdapter] = useState<AdapterFilter>('all')
  const showProject = !projectId

  if (executions.length === 0) {
    return (
      <EmptyState
        icon={ActivityIcon}
        title="No executions yet"
        description={
          projectId
            ? 'Accept a proposal for this project or start a dry run to rehearse a plan.'
            : 'Executions appear here once a proposal is accepted or a dry run is started.'
        }
        action={emptyAction}
      />
    )
  }

  const visible = adapter === 'all' ? executions : executions.filter((execution) => execution.adapter === adapter)

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <AdapterFilterGroup value={adapter} onChange={setAdapter} counts={countByAdapter(executions)} />
        <span className="text-xs tabular-nums text-muted-foreground">
          {visible.length} of {executions.length}
        </span>
      </div>
      {visible.length === 0 ? (
        <EmptyState
          icon={FilterXIcon}
          title={`No ${adapterLabel(adapter)} executions`}
          description="Nothing has run on this adapter yet. Clear the filter to see every execution."
          action={
            <Button variant="outline" size="sm" onClick={() => setAdapter('all')}>
              Show all
            </Button>
          }
        />
      ) : (
        <div className="overflow-hidden rounded-lg ring-1 ring-border">
          <Table aria-busy={query.isFetchingNextPage || undefined}>
            <TableHeader className="bg-muted/40">
              <TableRow className="hover:bg-transparent">
                <TableHead className="h-10 text-xs">#</TableHead>
                <TableHead className="h-10 text-xs">Title</TableHead>
                {showProject && <TableHead className="h-10 text-xs">Project</TableHead>}
                <TableHead className="h-10 text-xs">Status</TableHead>
                <TableHead className="h-10 text-xs">Progress</TableHead>
                <TableHead className="h-10 text-xs">Adapter</TableHead>
                <TableHead className="h-10 text-right text-xs">Updated</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {visible.map((execution) => (
                <ExecutionRow key={execution.id} execution={execution} showProject={showProject} />
              ))}
            </TableBody>
          </Table>
          {query.hasNextPage ? (
            <FrameFooter className="justify-between">
              <span className="tabular-nums">{count(executions.length, 'execution', 'executions')} loaded, more before</span>
              <Button variant="ghost" size="xs" onClick={() => query.fetchNextPage()} loading={query.isFetchingNextPage}>
                Load earlier
              </Button>
            </FrameFooter>
          ) : null}
        </div>
      )}
    </div>
  )
}
