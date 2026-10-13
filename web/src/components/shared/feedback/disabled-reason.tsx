import type { ReactNode } from 'react'

import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

interface DisabledReasonProps {
  /** Why the control inside is off; null when it is live, and then nothing is added around it. */
  reason: string | null
  children: ReactNode
}

/**
 * Says why a control is off. A disabled button fires no pointer events and
 * takes no focus, so this wrapper carries the reason as a tooltip and is
 * what the keyboard lands on instead.
 */
export function DisabledReason({ reason, children }: DisabledReasonProps) {
  if (!reason) return children
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span tabIndex={0} className="inline-flex rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
          {children}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-64">{reason}</TooltipContent>
    </Tooltip>
  )
}
