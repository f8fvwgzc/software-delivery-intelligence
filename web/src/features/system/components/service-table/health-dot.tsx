import type { StatusTone } from '@/components/shared/data'
import { cn } from '@/lib/utils'

import { DOT_CLASS } from '../../lib/labels'

interface HealthDotProps {
  tone: StatusTone
  className?: string
}

/** 6px status dot; the label next to it carries the meaning. */
export function HealthDot({ tone, className }: HealthDotProps) {
  return <span aria-hidden className={cn('inline-block size-1.5 shrink-0 rounded-full', DOT_CLASS[tone], className)} />
}
