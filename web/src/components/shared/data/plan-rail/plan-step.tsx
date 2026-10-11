import { ArrowRightIcon } from 'lucide-react'

import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { ActionTypeChip } from '../action-type-chip'
import { riskOf } from '../risk'
import { RiskDot } from '../risk-dot'

type PlanStep = components['schemas']['PlanStep']

interface PlanRailStepProps {
  step: PlanStep
  position: number
  /** Step key → title, to print dependencies by name. */
  titles: Map<string, string>
  compact: boolean
}

export function PlanRailStep({ step, position, titles, compact }: PlanRailStepProps) {
  const risk = riskOf(step.actionType)

  return (
    <li className={cn('relative flex flex-col', compact ? 'gap-1 py-2.5' : 'gap-1.5 py-3')}>
      <span
        aria-hidden="true"
        className={cn(
          'absolute -left-7 flex size-6 items-center justify-center rounded-full bg-background font-mono text-2xs tabular-nums text-muted-foreground ring-1 ring-border',
          compact ? 'top-2.5' : 'top-3',
        )}
      >
        {position}
      </span>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="text-sm font-medium leading-5">{step.title}</span>
        <ActionTypeChip variant="outline" className="text-muted-foreground">
          {step.actionType}
        </ActionTypeChip>
        <RiskDot risk={risk} withLabel className="text-2xs" />
        <span className="text-2xs text-muted-foreground">
          via <span className="font-mono">{step.adapter}</span>
        </span>
      </div>
      {step.description ? <p className="text-xs text-muted-foreground">{step.description}</p> : null}
      <dl className={cn('flex flex-col text-2xs text-muted-foreground', compact ? 'gap-0.5' : 'gap-1')}>
        {step.dependsOn.length > 0 ? (
          <div className="flex flex-wrap items-center gap-x-1.5">
            <dt>After</dt>
            {step.dependsOn.map((key, index) => (
              <dd key={key} className="inline-flex items-center gap-1">
                {index > 0 ? <span aria-hidden="true">,</span> : null}
                <span>{titles.get(key) ?? key}</span>
                <ArrowRightIcon className="size-3" aria-hidden="true" />
              </dd>
            ))}
            <dd>this step</dd>
          </div>
        ) : null}
        {step.requiredPermissions.length > 0 ? (
          <div className="flex flex-wrap items-center gap-x-1.5">
            <dt>Needs</dt>
            {step.requiredPermissions.map((permission) => (
              <dd key={permission} className="font-mono">
                {permission}
              </dd>
            ))}
          </div>
        ) : null}
        {!compact && step.verification ? (
          <div className="flex gap-1.5">
            <dt className="shrink-0">Verified by</dt>
            <dd>{step.verification}</dd>
          </div>
        ) : null}
      </dl>
    </li>
  )
}
