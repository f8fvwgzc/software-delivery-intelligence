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
 * The project's signature bet: a vertical step rail. It shows a plan that
 * would be executed, in order, with each step's risk and gates. Proposal
 * options and pattern templates share it so a plan reads the same wherever
 * it is previewed.
 */
export function PlanRail({ steps, emptyMessage = 'This option has no plan steps.', compact = false }: PlanRailProps) {
  if (steps.length === 0) {
    return <p className="text-xs text-muted-foreground">{emptyMessage}</p>
  }

  const titles = new Map(steps.map((step) => [step.key, step.title]))

  return (
    <ol className="relative flex flex-col gap-0 pl-7 before:absolute before:top-3 before:bottom-3 before:left-[11px] before:w-px before:bg-border">
      {steps.map((step, index) => (
        <PlanRailStep key={step.key} step={step} position={index + 1} titles={titles} compact={compact} />
      ))}
    </ol>
  )
}
