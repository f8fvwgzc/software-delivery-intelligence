import { StatusBadge } from '@/components/shared/data'
import { TableCell, TableRow } from '@/components/ui/table'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { formatDateTime, formatShortDateTime } from '@/lib/intl'

import type { AuditEntry } from '../../api/types'
import { actionTone } from '../../lib/labels'

function shortId(id: string) {
  return id.length > 12 ? `${id.slice(0, 8)}…` : id
}

/** Month, day and clock in mono, the full timestamp on hover. */
function Time({ value }: { value: string }) {
  if (Number.isNaN(new Date(value).getTime())) return <span className="font-mono text-xs text-muted-foreground">{value}</span>
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <time dateTime={value} className="font-mono text-xs tabular-nums text-muted-foreground">
          {formatShortDateTime(value)}
        </time>
      </TooltipTrigger>
      <TooltipContent>{formatDateTime(value)}</TooltipContent>
    </Tooltip>
  )
}

export function AuditRow({ entry }: { entry: AuditEntry }) {
  const actor = entry.userLogin || (entry.userId === 'system' ? 'system' : shortId(entry.userId))

  return (
    <TableRow>
      <TableCell className="whitespace-nowrap px-3 py-1.5">
        <Time value={entry.at} />
      </TableCell>
      <TableCell className="max-w-32 truncate px-3 py-1.5 font-mono text-xs">{actor}</TableCell>
      <TableCell className="px-3 py-1.5">
        <StatusBadge tone={actionTone(entry.action)} dot={false} className="font-mono text-2xs">
          {entry.action}
        </StatusBadge>
      </TableCell>
      <TableCell className="whitespace-nowrap px-3 py-1.5 font-mono text-xs">
        <span>{entry.entity}</span>
        {entry.entityId ? (
          <span className="text-muted-foreground" title={entry.entityId}>
            {' '}
            {shortId(entry.entityId)}
          </span>
        ) : null}
      </TableCell>
      <TableCell className="max-w-0 truncate px-3 py-1.5 text-xs" title={entry.detail || undefined}>
        {entry.detail || <span className="text-muted-foreground">—</span>}
      </TableCell>
      <TableCell className="hidden whitespace-nowrap px-3 py-1.5 font-mono text-2xs text-muted-foreground lg:table-cell">
        {entry.requestId ? <span title={entry.requestId}>{shortId(entry.requestId)}</span> : '—'}
      </TableCell>
    </TableRow>
  )
}
