import { cn } from '@/lib/utils'

import type { ProjectStatus as ProjectStatusValue } from '../../api/queries'
import { PROJECT_STATUS } from '../../lib/labels'

const DOT_CLASS = {
  neutral: 'bg-muted-foreground/60',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
} as const

interface ProjectStatusProps {
  status: ProjectStatusValue
  className?: string
}

/**
 * 6px status dot plus its label, for table rows. The dot pulses while a
 * scan is running: the one place motion is allowed in a row.
 */
export function ProjectStatus({ status, className }: ProjectStatusProps) {
  const { label, tone, live } = PROJECT_STATUS[status]

  return (
    <span className={cn('inline-flex items-center gap-2', className)}>
      <span className="relative flex size-1.5">
        {live ? (
          <span
            className={cn(
              'absolute inline-flex size-full rounded-full opacity-60 motion-safe:animate-ping',
              DOT_CLASS[tone],
            )}
          />
        ) : null}
        <span className={cn('relative inline-flex size-1.5 rounded-full', DOT_CLASS[tone])} />
      </span>
      <span>{label}</span>
    </span>
  )
}
