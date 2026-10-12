import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Person, RelativeTime, StatusBadge } from '@/components/shared/data'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'

import { APPROVAL_STATUS } from '../../lib/labels'

interface HistoryTableProps {
  approvals: components['schemas']['Approval'][]
}

export function HistoryTable({ approvals }: HistoryTableProps) {
  if (approvals.length === 0) {
    return (
      <p className="rounded-lg border border-dashed px-4 py-6 text-center text-xs text-muted-foreground">
        No decisions recorded yet.
      </p>
    )
  }

  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader className="bg-muted/40">
          <TableRow className="hover:bg-transparent">
            <TableHead className="h-9 text-xs">Execution</TableHead>
            <TableHead className="h-9 text-xs">Step</TableHead>
            <TableHead className="h-9 text-xs">Decision</TableHead>
            <TableHead className="h-9 text-xs">By</TableHead>
            <TableHead className="h-9 text-xs">Note</TableHead>
            <TableHead className="h-9 text-right text-xs">Decided</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {approvals.map((approval) => {
            const status = APPROVAL_STATUS[approval.status]
            return (
              <TableRow key={approval.id}>
                <TableCell className="max-w-56">
                  <Link to={routes.execution(approval.executionId)} className="inline-flex max-w-full items-baseline gap-1 hover:underline">
                    <span className="font-mono text-xs tabular-nums text-muted-foreground">#{approval.executionNumber}</span>
                    <span className="truncate">{approval.executionTitle}</span>
                  </Link>
                </TableCell>
                <TableCell className="max-w-56">
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate">{approval.stepTitle}</span>
                    <span className="font-mono text-2xs text-muted-foreground">{approval.actionType}</span>
                  </div>
                </TableCell>
                <TableCell>
                  <StatusBadge tone={status.tone}>{status.label}</StatusBadge>
                </TableCell>
                <TableCell className="max-w-40 text-xs text-muted-foreground">
                  <Person value={approval.decidedBy} />
                </TableCell>
                <TableCell className="max-w-64 truncate text-xs text-muted-foreground">{approval.note || '—'}</TableCell>
                <TableCell className="text-right text-xs text-muted-foreground">
                  <RelativeTime value={approval.decidedAt ?? approval.requestedAt} className="font-mono tabular-nums" />
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}

export function HistoryTableSkeleton({ rows = 4 }: { rows?: number }) {
  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <div className="flex h-9 items-center gap-3 border-b bg-muted/40 px-3">
        <Skeleton className="h-3 w-24" />
        <Skeleton className="h-3 w-24" />
        <Skeleton className="h-3 w-16" />
        <Skeleton className="ml-auto h-3 w-14" />
      </div>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="flex h-11 items-center gap-3 border-b px-3 last:border-0">
          <Skeleton className="h-3 w-40" />
          <Skeleton className="h-3 w-32" />
          <Skeleton className="h-5 w-20 rounded-md" />
          <Skeleton className="ml-auto h-3 w-16" />
        </div>
      ))}
    </div>
  )
}
