import { format, formatDistanceToNowStrict, isValid, parseISO } from 'date-fns'

import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

interface RelativeTimeProps {
  /** ISO 8601 string, or nothing. */
  value?: string | null
  /** Shown when there is no value. */
  fallback?: string
  className?: string
}

/** "3 minutes ago" with the exact time on hover. */
export function RelativeTime({ value, fallback = '—', className }: RelativeTimeProps) {
  if (!value) return <span className={className}>{fallback}</span>

  const date = parseISO(value)
  if (!isValid(date)) return <span className={className}>{fallback}</span>

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <time dateTime={value} className={className}>
          {formatDistanceToNowStrict(date, { addSuffix: true })}
        </time>
      </TooltipTrigger>
      <TooltipContent>{format(date, 'PPpp')}</TooltipContent>
    </Tooltip>
  )
}
