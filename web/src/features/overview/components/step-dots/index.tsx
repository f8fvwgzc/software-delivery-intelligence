import { cn } from '@/lib/utils'

import type { StepState } from '../../lib/work-queue'

const STATE_CLASS: Record<StepState, string> = {
  done: 'bg-foreground/60',
  failed: 'bg-destructive',
  blocked: 'ring-1 ring-inset ring-border bg-transparent',
  pending: 'bg-muted-foreground/25',
}

const STATE_LABEL: Record<StepState, string> = {
  done: 'checkpointed',
  failed: 'failed',
  blocked: 'blocked',
  pending: 'pending',
}

interface StepDotsProps {
  states: StepState[]
  /** Dots past this are summed into "+n". */
  max?: number
  className?: string
}

/**
 * The execution rail at its smallest: one 6px dot per step, in plan order.
 * Filled = checkpointed, red = failed, hollow = blocked by the failure.
 */
export function StepDots({ states, max = 8, className }: StepDotsProps) {
  if (states.length === 0) return null

  const visible = states.slice(0, max)
  const hidden = states.length - visible.length
  const done = states.filter((state) => state === 'done').length
  const summary = `${done} of ${states.length} steps checkpointed`

  return (
    <span className={cn('inline-flex items-center gap-1', className)} role="img" aria-label={summary} title={summary}>
      {visible.map((state, index) => (
        <span key={index} className={cn('size-1.5 rounded-full', STATE_CLASS[state])} data-state={STATE_LABEL[state]} />
      ))}
      {hidden > 0 ? <span className="text-2xs tabular-nums text-muted-foreground">+{hidden}</span> : null}
    </span>
  )
}
