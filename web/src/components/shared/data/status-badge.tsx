import type { ReactNode } from 'react'

import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

export type StatusTone = 'neutral' | 'info' | 'success' | 'warning' | 'destructive' | 'brand'

const TONE_CLASS: Record<StatusTone, string> = {
  neutral: 'border-border bg-muted text-muted-foreground',
  info: 'border-info/20 bg-info/10 text-info',
  success: 'border-success/20 bg-success/10 text-success',
  warning: 'border-warning/25 bg-warning/10 text-warning',
  destructive: 'border-destructive/20 bg-destructive/10 text-destructive',
  brand: 'border-brand/20 bg-brand/10 text-brand',
}

const DOT_CLASS: Record<StatusTone, string> = {
  neutral: 'bg-muted-foreground/60',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
}

interface StatusBadgeProps {
  tone: StatusTone
  children: ReactNode
  /** Pulses the dot for in-progress states. */
  live?: boolean
  dot?: boolean
  className?: string
}

/** Tinted badge with an optional status dot. Use for every lifecycle state. */
export function StatusBadge({ tone, children, live = false, dot = true, className }: StatusBadgeProps) {
  return (
    <Badge variant="outline" className={cn('gap-1.5 font-normal', TONE_CLASS[tone], className)}>
      {dot && (
        <span className="relative flex size-1.5">
          {live && (
            <span className={cn('absolute inline-flex size-full animate-ping rounded-full opacity-60', DOT_CLASS[tone])} />
          )}
          <span className={cn('relative inline-flex size-1.5 rounded-full', DOT_CLASS[tone])} />
        </span>
      )}
      {children}
    </Badge>
  )
}
