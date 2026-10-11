import { ScrollTextIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { AuditEntry } from '../../api/types'
import { AuditRow } from './audit-row'

export { AuditTableSkeleton } from './skeleton'

const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

interface AuditTableProps {
  entries: AuditEntry[]
  /** The prefix the rows answer to; drives the empty copy. */
  actionPrefix: string
  hasEarlier: boolean
  loadingEarlier: boolean
  onLoadEarlier: () => void
  onClearFilter: () => void
}

/** Newest first, one line per entry, "Load earlier" at the foot. */
export function AuditTable({ entries, actionPrefix, hasEarlier, loadingEarlier, onLoadEarlier, onClearFilter }: AuditTableProps) {
  if (entries.length === 0) {
    return actionPrefix ? (
      <EmptyState
        icon={ScrollTextIcon}
        title={`No entries start with “${actionPrefix}”`}
        description="Actions are dotted keys such as project.connect or approval.decide."
        action={
          <Button variant="outline" size="sm" onClick={onClearFilter}>
            Show all
          </Button>
        }
      />
    ) : (
      <EmptyState
        icon={ScrollTextIcon}
        title="Nothing audited yet"
        description="Connecting a project, deciding an approval or changing a policy writes the first entry."
      />
    )
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="overflow-hidden rounded-lg ring-1 ring-border">
        <Table>
          <TableHeader>
            <TableRow className="bg-muted/30 hover:bg-muted/30">
              <TableHead className={HEAD_CLASS}>Time</TableHead>
              <TableHead className={HEAD_CLASS}>Actor</TableHead>
              <TableHead className={HEAD_CLASS}>Action</TableHead>
              <TableHead className={HEAD_CLASS}>Entity</TableHead>
              <TableHead className={HEAD_CLASS}>Detail</TableHead>
              <TableHead className={`${HEAD_CLASS} hidden lg:table-cell`}>Request</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {entries.map((entry) => (
              <AuditRow key={entry.id} entry={entry} />
            ))}
          </TableBody>
        </Table>
      </div>
      <div className="flex items-center justify-between gap-3">
        <span className="text-xs tabular-nums text-muted-foreground">
          {entries.length} {entries.length === 1 ? 'entry' : 'entries'}
          {hasEarlier ? ', more before' : ''}
        </span>
        {hasEarlier ? (
          <Button variant="outline" size="sm" loading={loadingEarlier} onClick={onLoadEarlier}>
            Load earlier
          </Button>
        ) : null}
      </div>
    </div>
  )
}
