import { cn } from '@/lib/utils'

import type { StepProgress } from '../../lib/progress'

interface ProgressBarProps {
  progress: StepProgress
  className?: string
}

const SEGMENTS: { key: keyof Omit<StepProgress, 'total'>; className: string }[] = [
  { key: 'succeeded', className: 'bg-success/70' },
  { key: 'running', className: 'bg-brand/70' },
  { key: 'failed', className: 'bg-destructive/70' },
  { key: 'blocked', className: 'bg-warning/50' },
  { key: 'pending', className: 'bg-muted-foreground/15' },
]

/** Thin segmented bar: one segment per outcome, widths proportional to step count. */
export function ProgressBar({ progress, className }: ProgressBarProps) {
  if (progress.total === 0) return <div className={cn('h-1 rounded-full bg-muted', className)} />

  return (
    <div
      role="img"
      aria-label={`${progress.succeeded} of ${progress.total} steps succeeded`}
      className={cn('flex h-1 w-full gap-px overflow-hidden rounded-full', className)}
    >
      {SEGMENTS.map(({ key, className: segmentClass }) => {
        const count = progress[key]
        if (count === 0) return null
        return (
          <span
            key={key}
            className={cn('h-full transition-[flex-grow] duration-300 ease-out motion-reduce:transition-none', segmentClass)}
            style={{ flexGrow: count }}
          />
        )
      })}
    </div>
  )
}
