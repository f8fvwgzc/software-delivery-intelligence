import { CheckIcon, ClockIcon, LockIcon, MinusIcon, XIcon } from 'lucide-react'
import type { ComponentProps, ReactNode } from 'react'

import { Spinner } from '@/components/ui/spinner'
import { cn } from '@/lib/utils'

export type RailStatus = 'pending' | 'ready' | 'running' | 'waiting' | 'done' | 'failed' | 'skipped' | 'blocked'

const MARKER_CLASS: Record<RailStatus, string> = {
  pending: 'bg-background text-muted-foreground ring-border',
  ready: 'bg-background text-info ring-info/40',
  running: 'bg-brand/10 text-brand ring-brand/40',
  waiting: 'bg-warning/10 text-warning ring-warning/40',
  done: 'bg-success/10 text-success ring-success/40',
  failed: 'bg-destructive/10 text-destructive ring-destructive/40',
  skipped: 'bg-muted text-muted-foreground ring-border',
  blocked: 'bg-muted text-muted-foreground ring-border',
}

export const RAIL_STATUS_LABEL: Record<RailStatus, string> = {
  pending: 'Pending',
  ready: 'Ready',
  running: 'Running',
  waiting: 'Waiting for a decision',
  done: 'Done',
  failed: 'Failed',
  skipped: 'Skipped',
  blocked: 'Blocked',
}

function MarkerIcon({ status }: { status: RailStatus }) {
  switch (status) {
    case 'done':
      return <CheckIcon className="size-3" strokeWidth={2.25} />
    case 'running':
      return <Spinner className="size-3" />
    case 'failed':
      return <XIcon className="size-3" strokeWidth={2.25} />
    case 'waiting':
      return <ClockIcon className="size-3" />
    case 'blocked':
      return <LockIcon className="size-2.5" />
    case 'skipped':
      return <MinusIcon className="size-2.5" />
    default:
      return <span className="size-1.5 rounded-full bg-current" />
  }
}

/** 20px marker on the rail, its colour and glyph set by the status; `children` replaces the glyph (a step number). */
export function RailMarker({
  status,
  label,
  className,
  children,
}: {
  status: RailStatus
  label?: string
  className?: string
  children?: ReactNode
}) {
  return (
    <span
      role="img"
      aria-label={label ?? RAIL_STATUS_LABEL[status]}
      data-status={status}
      className={cn(
        'relative z-10 flex size-5 shrink-0 items-center justify-center rounded-full ring-1 transition-[color,background-color,box-shadow] duration-(--motion-base) ease-(--ease-standard)',
        MARKER_CLASS[status],
        className,
      )}
    >
      {children ?? <MarkerIcon status={status} />}
    </span>
  )
}

/**
 * The vertical step rail, the app's signature structure. One hairline runs
 * behind the markers; executions, reviews, scans and plan previews all draw
 * their steps on it so a sequence reads the same everywhere.
 */
export function Rail({ className, ...props }: ComponentProps<'ol'>) {
  return <ol data-slot="rail" className={cn('relative flex flex-col', className)} {...props} />
}

export interface RailStepProps {
  status: RailStatus
  /** Marker label for assistive tech; defaults to the status. */
  statusLabel?: string
  title: ReactNode
  /** Tags and small text on the title line. */
  meta?: ReactNode
  /** Right end of the title line: a status tag, a duration. */
  trailing?: ReactNode
  /** Muted line under the title. */
  detail?: ReactNode
  /** Expanded content under the step (a failure panel, an inline approval). */
  children?: ReactNode
  /** Marks the step as the one in focus: a tinted ring around its body. */
  emphasis?: 'none' | 'running' | 'failed' | 'waiting'
  /** Small label under the marker (`checkpoint`). */
  badge?: ReactNode
  /** Replaces the marker glyph: a step number in plan previews. */
  marker?: ReactNode
  /** Dim pending/skipped steps. */
  dimmed?: boolean
  /** Rows without the framed body, for compact previews. */
  compact?: boolean
  className?: string
}

const EMPHASIS_CLASS = {
  none: '',
  running: 'ring-brand/30',
  failed: 'ring-destructive/30',
  waiting: 'ring-warning/30',
}

export function RailStep({
  status,
  statusLabel,
  title,
  meta,
  trailing,
  detail,
  children,
  emphasis = 'none',
  badge,
  marker,
  dimmed = false,
  compact = false,
  className,
}: RailStepProps) {
  return (
    <li
      data-slot="rail-step"
      data-status={status}
      className={cn(
        'relative flex gap-3 pb-4 last:pb-0',
        'after:absolute after:top-6 after:bottom-0 after:left-[9px] after:w-px after:bg-border last:after:hidden',
        compact && 'pb-3',
        className,
      )}
    >
      <div className="relative flex w-5 shrink-0 flex-col items-center pt-0.5">
        <RailMarker status={status} label={statusLabel}>
          {marker}
        </RailMarker>
        {badge ? <span className="absolute top-7 left-1/2 z-10 -translate-x-1/2 whitespace-nowrap">{badge}</span> : null}
      </div>
      <div
        className={cn(
          'flex min-w-0 flex-1 flex-col gap-2',
          !compact && 'rounded-md px-3 py-2 ring-1 ring-border',
          !compact && EMPHASIS_CLASS[emphasis],
          dimmed && 'opacity-60',
        )}
      >
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <div className={cn('flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1 text-sm leading-5', emphasis === 'failed' ? 'font-semibold' : 'font-medium')}>
            {title}
          </div>
          {meta}
          {trailing ? <span className="ml-auto flex shrink-0 items-center gap-2">{trailing}</span> : null}
        </div>
        {detail ? <div className="text-xs leading-4 text-muted-foreground">{detail}</div> : null}
        {children}
      </div>
    </li>
  )
}
