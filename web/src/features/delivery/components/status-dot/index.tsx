import type { StatusTone } from '@/components/shared/data'
import { cn } from '@/lib/utils'

import { TONE_DOT_CLASS } from '../../lib/labels'

interface StatusDotProps {
  tone: StatusTone
  label: string
  live?: boolean
  className?: string
}

/** Table-weight status: a 6px dot and the label, no badge chrome. */
export function StatusDot({ tone, label, live = false, className }: StatusDotProps) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 whitespace-nowrap text-xs', className)}>
      <span className="relative flex size-1.5 shrink-0">
        {live ? (
          <span
            className={cn(
              'absolute inline-flex size-full animate-ping rounded-full opacity-60 motion-reduce:animate-none',
              TONE_DOT_CLASS[tone],
            )}
          />
        ) : null}
        <span className={cn('relative inline-flex size-1.5 rounded-full', TONE_DOT_CLASS[tone])} />
      </span>
      {label}
    </span>
  )
}
