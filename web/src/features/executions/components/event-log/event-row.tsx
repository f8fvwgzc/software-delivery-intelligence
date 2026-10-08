import { format, isValid, parseISO } from 'date-fns'

import type { StatusTone } from '@/components/shared/data'
import { cn } from '@/lib/utils'

import { EVENT_TYPE } from '../../lib/labels'
import type { ExecutionEvent } from './use-event-stream'

const TYPE_CLASS: Record<StatusTone, string> = {
  neutral: 'text-muted-foreground',
  info: 'text-info',
  success: 'text-success',
  warning: 'text-warning',
  destructive: 'text-destructive',
  brand: 'text-brand',
}

function clock(iso: string) {
  const date = parseISO(iso)
  return isValid(date) ? format(date, 'HH:mm:ss') : '--:--:--'
}

interface EventRowProps {
  event: ExecutionEvent
}

/** One dense mono line: seq, time, type, message, step. */
export function EventRow({ event }: EventRowProps) {
  const type = EVENT_TYPE[event.type]

  return (
    <li className="grid grid-cols-[2.25rem_3.75rem_4.5rem_minmax(0,1fr)] items-baseline gap-x-2 px-3 py-1 font-mono text-xs leading-5 hover:bg-muted/40">
      <span className="text-right tabular-nums text-muted-foreground/70">{event.seq}</span>
      <span className="tabular-nums text-muted-foreground">{clock(event.at)}</span>
      <span className={cn('truncate', TYPE_CLASS[type.tone])}>{type.label}</span>
      <span className="min-w-0 break-words text-foreground/90">
        {event.message}
        {event.stepKey && <span className="ml-1.5 text-muted-foreground/70">[{event.stepKey}]</span>}
      </span>
    </li>
  )
}
