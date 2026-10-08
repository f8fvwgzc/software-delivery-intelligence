import type { components } from '@/lib/api/schema'

import { PlanRailStep } from './plan-step'

type PlanStep = components['schemas']['PlanStep']

interface PlanRailProps {
  steps: PlanStep[]
}

/**
 * The project's signature bet: a vertical step rail. Here it shows the
 * plan that would be executed, in order, with each step's risk and gates.
 */
export function PlanRail({ steps }: PlanRailProps) {
  if (steps.length === 0) {
    return <p className="text-xs text-muted-foreground">This option has no plan steps.</p>
  }

  const titles = new Map(steps.map((step) => [step.key, step.title]))

  return (
    <ol className="relative flex flex-col gap-0 pl-7 before:absolute before:top-3 before:bottom-3 before:left-[11px] before:w-px before:bg-border">
      {steps.map((step, index) => (
        <PlanRailStep key={step.key} step={step} position={index + 1} titles={titles} />
      ))}
    </ol>
  )
}
