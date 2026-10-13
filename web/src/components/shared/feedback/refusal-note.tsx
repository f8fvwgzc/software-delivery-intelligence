import { CircleAlertIcon } from 'lucide-react'

import { ApiError } from '@/lib/api/errors'
import { cn } from '@/lib/utils'

import { describeError } from './error-state'

/**
 * Why a request was not carried out, where the person asked for it. A 412
 * is the server's own sentence and says what to do next (turn a switch on,
 * review the new head, wait for a check), so it is shown word for word;
 * anything else names the failure.
 */
export function RefusalNote({ error, className }: { error: unknown; className?: string }) {
  const refused = error instanceof ApiError && error.status === 412
  const described = describeError(error)

  return (
    <p role="alert" className={cn('flex items-start gap-1.5 text-xs leading-4', refused ? 'text-warning' : 'text-destructive', className)}>
      <CircleAlertIcon className="mt-px size-3.5 shrink-0" aria-hidden />
      <span>{refused ? error.message : `${described.title}. ${described.description}`}</span>
    </p>
  )
}
