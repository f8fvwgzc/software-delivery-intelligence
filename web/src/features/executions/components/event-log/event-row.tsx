import { memo, type CSSProperties } from 'react'

import type { StatusTone } from '@/components/shared/data/status-badge'
import { formatClock } from '@/lib/intl'
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

/** One line of 20px leading plus 4px padding each side; messages that wrap are measured. */
export const EVENT_ROW_HEIGHT = 28

interface EventRowProps {
  event: ExecutionEvent
  /** Position in the windowed list, for the measurer. */
  index?: number
  measure?: (element: HTMLElement | null) => void
  style?: CSSProperties
}

/** One dense mono line: seq, time, type, message, step. */
export const EventRow = memo(function EventRow({ event, index, measure, style }: EventRowProps) {
  const type = EVENT_TYPE[event.type]

  return (
    <li
      data-index={index}
      ref={measure}
      style={style}
      className="grid grid-cols-[2.25rem_3.75rem_4.5rem_minmax(0,1fr)] items-baseline gap-x-2 px-3 py-1 font-mono text-xs leading-5 hover:bg-muted/40"
    >
      <span className="text-right tabular-nums text-muted-foreground">{event.seq}</span>
      <span className="tabular-nums text-muted-foreground">{formatClock(event.at)}</span>
      <span className={cn('truncate', TYPE_CLASS[type.tone])}>{type.label}</span>
      <span className="min-w-0 break-words text-foreground/90">
        {event.message}
        {event.stepKey && <span className="ml-1.5 text-muted-foreground">[{event.stepKey}]</span>}
      </span>
    </li>
  )
})
