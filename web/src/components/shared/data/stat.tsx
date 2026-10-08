import type { ComponentType, ReactNode } from 'react'

import { Card } from '@/components/ui/card'
import { cn } from '@/lib/utils'

interface StatProps {
  label: ReactNode
  value: ReactNode
  /** Small line under the value, e.g. "of 12 runs". */
  hint?: ReactNode
  icon?: ComponentType<{ className?: string }>
  tone?: 'default' | 'success' | 'warning' | 'destructive' | 'brand'
  className?: string
}

const TONE_CLASS: Record<NonNullable<StatProps['tone']>, string> = {
  default: 'text-foreground',
  success: 'text-success',
  warning: 'text-warning',
  destructive: 'text-destructive',
  brand: 'text-brand',
}

/** A single number with a label. Lay several out in a grid. */
export function Stat({ label, value, hint, icon: Icon, tone = 'default', className }: StatProps) {
  return (
    <Card size="sm" className={cn('gap-1.5 px-3 py-3', className)}>
      <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
        <span className="truncate">{label}</span>
        {Icon && <Icon className="size-3.5 shrink-0" />}
      </div>
      <div className={cn('text-xl font-semibold tabular-nums leading-tight', TONE_CLASS[tone])}>{value}</div>
      {hint && <div className="text-2xs text-muted-foreground">{hint}</div>}
    </Card>
  )
}
