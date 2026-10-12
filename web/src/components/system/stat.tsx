import type { ComponentType, ReactNode } from 'react'

import { cn } from '@/lib/utils'

export interface StatProps {
  label: ReactNode
  value: ReactNode
  /** Plain secondary text under the value: "of 12 runs", "+3 this week". Never a coloured pill. */
  delta?: ReactNode
  icon?: ComponentType<{ className?: string }>
  tone?: 'default' | 'success' | 'warning' | 'destructive' | 'brand'
  /** Hairline box around the stat; off inside a Frame body. */
  framed?: boolean
  /** Larger value for the one hero number of a page. */
  size?: 'md' | 'lg'
  className?: string
}

const TONE_CLASS: Record<NonNullable<StatProps['tone']>, string> = {
  default: 'text-foreground',
  success: 'text-success',
  warning: 'text-warning',
  destructive: 'text-destructive',
  brand: 'text-brand',
}

/**
 * One number with its label: label in 11px small caps, value in tabular
 * figures, delta as plain muted text. Several side by side make a strip.
 */
export function Stat({ label, value, delta, icon: Icon, tone = 'default', framed = false, size = 'md', className }: StatProps) {
  return (
    <div
      data-slot="stat"
      className={cn('flex min-w-0 flex-col gap-1', framed && 'rounded-lg px-3 py-2.5 ring-1 ring-border', className)}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-2xs font-medium tracking-wide text-muted-foreground [font-variant-caps:all-small-caps]">{label}</span>
        {Icon ? <Icon className="size-3.5 shrink-0 text-muted-foreground" /> : null}
      </div>
      <div className={cn('font-semibold tabular-nums leading-none tracking-tight', size === 'lg' ? 'text-2xl' : 'text-lg', TONE_CLASS[tone])}>
        {value}
      </div>
      {delta ? <div className="truncate text-xs text-muted-foreground">{delta}</div> : null}
    </div>
  )
}

/** Stats in a row, separated by hairlines. */
export function StatStrip({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      data-slot="stat-strip"
      className={cn('grid grid-cols-2 gap-px overflow-hidden rounded-lg bg-border ring-1 ring-border sm:grid-cols-4 [&>[data-slot=stat]]:bg-background [&>[data-slot=stat]]:px-3 [&>[data-slot=stat]]:py-2.5', className)}
    >
      {children}
    </div>
  )
}
