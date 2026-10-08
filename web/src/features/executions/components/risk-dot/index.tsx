import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { RISK } from '../../lib/labels'

interface RiskDotProps {
  risk: components['schemas']['RiskLevel']
  /** Render the label next to the dot instead of in a tooltip. */
  withLabel?: boolean
  className?: string
}

/** 6px status dot for a risk level. Colour is never the only signal: the label is always reachable. */
export function RiskDot({ risk, withLabel = false, className }: RiskDotProps) {
  const meta = RISK[risk]
  const dot = <span aria-hidden className={cn('inline-block size-1.5 shrink-0 rounded-full', meta.dot)} />

  if (withLabel) {
    return (
      <span className={cn('inline-flex items-center gap-1.5 text-xs text-muted-foreground', className)}>
        {dot}
        {meta.label}
      </span>
    )
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className={cn('inline-flex size-4 items-center justify-center', className)} aria-label={meta.label}>
          {dot}
        </span>
      </TooltipTrigger>
      <TooltipContent>{meta.label}</TooltipContent>
    </Tooltip>
  )
}
