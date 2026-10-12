import type { ComponentType, ReactNode } from 'react'

import { Stat as SystemStat } from '@/components/system/stat'

interface StatProps {
  label: ReactNode
  value: ReactNode
  /** Small line under the value, e.g. "of 12 runs". */
  hint?: ReactNode
  icon?: ComponentType<{ className?: string }>
  tone?: 'default' | 'success' | 'warning' | 'destructive' | 'brand'
  className?: string
}

/** A single number with a label, framed. Prefer the system `Stat` and `StatStrip` in new code. */
export function Stat({ label, value, hint, icon, tone = 'default', className }: StatProps) {
  return <SystemStat label={label} value={value} delta={hint} icon={icon} tone={tone} framed className={className} />
}
