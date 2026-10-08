import { cn } from '@/lib/utils'

import { formatCount } from '../../lib/format'

interface CountProps {
  value: number
  /** Pre-formatted text overrides the number formatting (rates, durations). */
  text?: string
  className?: string
}

/** A number in a key/value list: always tabular. */
export function Count({ value, text, className }: CountProps) {
  return <span className={cn('tabular-nums', className)}>{text ?? formatCount(value)}</span>
}
