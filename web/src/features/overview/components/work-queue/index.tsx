import { InboxIcon } from 'lucide-react'
import { useState } from 'react'

import { Frame } from '@/components/system/frame'
import { PanelEmpty } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { QUEUE_KIND } from '../../lib/labels'
import { countByKind, type QueueFilter, type QueueRow as QueueRowData } from '../../lib/work-queue'
import { QueueFilterGroup } from './queue-filter'
import { QueueRow } from './queue-row'
import { handleRowNavigation } from './row-navigation'

export { WorkQueueSkeleton } from './skeleton'

const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

/**
 * The operator's table: every row is something only a human can move.
 * Rows are reachable by Tab, and arrow keys / j k walk the actions.
 */
export function WorkQueue({ rows }: { rows: QueueRowData[] }) {
  const [filter, setFilter] = useState<QueueFilter>('all')
  const counts = countByKind(rows)
  const visible = filter === 'all' ? rows : rows.filter((row) => row.kind === filter)

  return (
    <Frame
      title="Work queue"
      description="Oldest first. Paused executions, approvals and open questions."
      actions={rows.length > 0 ? <QueueFilterGroup value={filter} counts={counts} onChange={setFilter} /> : null}
      padding="none"
    >
      {rows.length === 0 ? (
        <PanelEmpty
          icon={InboxIcon}
          title="Nothing is waiting on you"
          description="Approvals, paused executions and open questions show up here the moment they need a decision."
        />
      ) : visible.length === 0 ? (
        <PanelEmpty
          title={`No ${QUEUE_KIND[filter as Exclude<QueueFilter, 'all'>].label.toLowerCase()} rows`}
          description="The other kinds still need you."
          action={
            <Button variant="outline" size="sm" onClick={() => setFilter('all')}>
              Show everything
            </Button>
          }
        />
      ) : (
        <div>
          <Table onKeyDown={handleRowNavigation}>
            <TableHeader>
              <TableRow className="bg-muted/30 hover:bg-muted/30">
                <TableHead className={`${HEAD_CLASS} w-28`}>Kind</TableHead>
                <TableHead className={HEAD_CLASS}>Project</TableHead>
                <TableHead className={HEAD_CLASS}>What is needed</TableHead>
                <TableHead className={`${HEAD_CLASS} hidden w-32 sm:table-cell`}>Age</TableHead>
                <TableHead className={`${HEAD_CLASS} w-28 text-right`}>
                  <span className="sr-only">Action</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {visible.map((row) => (
                <QueueRow key={row.id} row={row} />
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </Frame>
  )
}
