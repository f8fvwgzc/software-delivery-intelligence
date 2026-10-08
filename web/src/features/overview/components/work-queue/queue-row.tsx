import { ArrowRightIcon } from 'lucide-react'
import { Link } from 'react-router'

import { RelativeTime } from '@/components/shared/data'
import { Button } from '@/components/ui/button'
import { TableCell, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

import { DOT_CLASS, QUEUE_KIND } from '../../lib/labels'
import type { QueueRow as QueueRowData } from '../../lib/work-queue'
import { StepDots } from '../step-dots'

export function QueueRow({ row }: { row: QueueRowData }) {
  const kind = QUEUE_KIND[row.kind]

  return (
    <TableRow className="focus-within:bg-muted/50">
      <TableCell className="px-3 py-2">
        <span className="flex items-center gap-2 text-xs">
          <span className="relative flex size-1.5 shrink-0">
            {row.live ? (
              <span
                aria-hidden
                className={cn(
                  'absolute inline-flex size-full animate-ping rounded-full opacity-60 motion-reduce:hidden',
                  DOT_CLASS[row.tone],
                )}
              />
            ) : null}
            <span aria-hidden className={cn('relative inline-flex size-1.5 rounded-full', DOT_CLASS[row.tone])} />
          </span>
          {kind.label}
        </span>
      </TableCell>
      <TableCell className="max-w-48 truncate px-3 py-2 font-medium">{row.projectName}</TableCell>
      <TableCell className="max-w-0 px-3 py-2">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate">{row.needed}</span>
          <span className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
            {row.steps ? <StepDots states={row.steps} /> : null}
            <span className="truncate" title={row.detail}>
              {row.detail}
            </span>
          </span>
        </div>
      </TableCell>
      <TableCell className="hidden px-3 py-2 font-mono text-xs text-muted-foreground sm:table-cell">
        <RelativeTime value={row.since} />
      </TableCell>
      <TableCell className="px-3 py-1.5 text-right">
        <Button asChild variant="ghost" size="sm" data-row-link>
          <Link to={row.to}>
            {kind.action}
            <ArrowRightIcon data-icon="inline-end" />
          </Link>
        </Button>
      </TableCell>
    </TableRow>
  )
}
