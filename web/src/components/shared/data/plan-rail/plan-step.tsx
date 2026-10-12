import { ArrowRightIcon } from 'lucide-react'

import { RailStep } from '@/components/system/rail'
import type { components } from '@/lib/api/schema'

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
    <RailStep
      status="pending"
      statusLabel={`Step ${position}`}
      marker={<span className="font-mono text-[10px] tabular-nums">{position}</span>}
      compact
      title={step.title}
      meta={
        <>
          <ActionTypeChip variant="outline" className="text-muted-foreground">
            {step.actionType}
          </ActionTypeChip>
          <RiskDot risk={risk} withLabel className="text-2xs" />
          <span className="text-2xs text-muted-foreground">
            via <span className="font-mono">{step.adapter}</span>
          </span>
        </>
      }
      detail={
        <div className="flex flex-col gap-0.5">
          {step.description ? <p>{step.description}</p> : null}
          <dl className="flex flex-col gap-0.5 text-2xs">
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
        </div>
      }
    />
  )
}
