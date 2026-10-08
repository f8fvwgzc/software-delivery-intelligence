import type { components } from '@/lib/api/schema'

import { PlanRailStep } from './plan-step'

type PlanStep = components['schemas']['PlanStep']

/**
 * The signature step rail, here for a pattern's plan template. Same shape
 * as the proposal's rail; features do not share components, so it lives
 * here too until the rail is promoted to components/shared.
 */
export function PlanRail({ steps }: { steps: PlanStep[] }) {
  if (steps.length === 0) {
    return <p className="text-xs text-muted-foreground">This pattern has no plan template.</p>
  }

  const titles = new Map(steps.map((step) => [step.key, step.title]))

  return (
    <ol className="relative flex flex-col pl-7 before:absolute before:top-3 before:bottom-3 before:left-[11px] before:w-px before:bg-border">
      {steps.map((step, index) => (
        <PlanRailStep key={step.key} step={step} position={index + 1} titles={titles} />
      ))}
    </ol>
  )
}
