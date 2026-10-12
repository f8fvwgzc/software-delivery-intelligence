import { useSuspenseInfiniteQuery } from '@tanstack/react-query'
import { HistoryIcon } from 'lucide-react'

import { FrameFooter } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { count } from '@/lib/intl'

import { cdQueries } from '../../api/queries'
import { RunRow } from './run-row'

const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

/**
 * Every promotion and rollback of the project, newest first, with its
 * status live while it runs. One page at a time; "Load earlier" at the foot.
 */
export function DeploymentRunsTable({ projectId }: { projectId: string }) {
  const query = useSuspenseInfiniteQuery(cdQueries.runs(projectId))
  const runs = query.data.pages.flat()

  if (runs.length === 0) {
    return (
      <PanelEmpty
        icon={HistoryIcon}
        title="No deployments yet"
        description="Promotions and rollbacks appear here, each one an execution with its own approvals, logs and checkpoints."
      />
    )
  }

  return (
    <>
      <Table aria-busy={query.isFetchingNextPage || undefined}>
        <TableHeader>
          <TableRow className="bg-muted/30 hover:bg-muted/30">
            <TableHead className={HEAD_CLASS}>Environment</TableHead>
            <TableHead className={HEAD_CLASS}>Release</TableHead>
            <TableHead className={HEAD_CLASS}>Kind</TableHead>
            <TableHead className={HEAD_CLASS}>Status</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden md:table-cell`}>Started by</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden sm:table-cell`}>Duration</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden lg:table-cell`}>Started</TableHead>
            <TableHead className={HEAD_CLASS}>
              <span className="sr-only">Links</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {runs.map((run) => (
            <RunRow key={run.id} run={run} />
          ))}
        </TableBody>
      </Table>
      {query.hasNextPage ? (
        <FrameFooter className="justify-between">
          <span className="tabular-nums">{count(runs.length, 'run', 'runs')} loaded, more before</span>
          <Button variant="ghost" size="xs" onClick={() => query.fetchNextPage()} loading={query.isFetchingNextPage}>
            Load earlier
          </Button>
        </FrameFooter>
      ) : null}
    </>
  )
}
