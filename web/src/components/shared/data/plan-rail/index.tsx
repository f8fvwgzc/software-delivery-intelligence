import { Rail } from '@/components/system/rail'
import type { components } from '@/lib/api/schema'

import { PlanRailStep } from './plan-step'

type PlanStep = components['schemas']['PlanStep']

interface PlanRailProps {
  steps: PlanStep[]
  /** Shown instead of the rail when the plan is empty. */
  emptyMessage?: string
  /** Tighter rows without the verification line, for catalog previews. */
  compact?: boolean
}

/**
 * A plan that would be executed, in order, with each step's risk and gates,
 * on the shared step rail. Proposal options and pattern templates share it
 * so a plan reads the same wherever it is previewed.
 */
export function PlanRail({ steps, emptyMessage = 'This option has no plan steps.', compact = false }: PlanRailProps) {
  if (steps.length === 0) {
    return <p className="text-xs text-muted-foreground">{emptyMessage}</p>
  }

  const titles = new Map(steps.map((step) => [step.key, step.title]))

  return (
    <Rail aria-label="Plan steps">
      {steps.map((step, index) => (
        <PlanRailStep key={step.key} step={step} position={index + 1} titles={titles} compact={compact} />
      ))}
    </Rail>
  )
}
