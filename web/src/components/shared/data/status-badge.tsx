import type { ReactNode } from 'react'

import { Tag, TAG_DOT_CLASS, type TagTone } from '@/components/system/tag'

export type StatusTone = TagTone

/** 6px dot fill per tone; StatusDot and the tag read the same. */
export const TONE_DOT_CLASS = TAG_DOT_CLASS

interface StatusBadgeProps {
  tone: StatusTone
  children: ReactNode
  /** Pulses the dot for in-progress states. */
  live?: boolean
  dot?: boolean
  className?: string
}

/** Lifecycle state as a system `Tag` with a status dot. */
export function StatusBadge({ tone, children, live = false, dot = true, className }: StatusBadgeProps) {
  return (
    <Tag tone={tone} dot={dot} live={live} className={className}>
      {children}
    </Tag>
  )
}
