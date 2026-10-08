import { ArrowRightIcon } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { RISK_LABEL, RISK_TONE, riskOf } from '../../lib/labels'

type PlanStep = components['schemas']['PlanStep']

const DOT_CLASS = {
  neutral: 'bg-muted-foreground/60',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
} as const

interface PlanRailStepProps {
  step: PlanStep
  position: number
  /** Step key → title, to print dependencies by name. */
  titles: Map<string, string>
}

export function PlanRailStep({ step, position, titles }: PlanRailStepProps) {
  const risk = riskOf(step.actionType)

  return (
    <li className="relative flex flex-col gap-1.5 py-3">
      <span
        aria-hidden="true"
        className="absolute top-3 -left-7 flex size-6 items-center justify-center rounded-full bg-background font-mono text-2xs tabular-nums text-muted-foreground ring-1 ring-border"
      >
        {position}
      </span>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className="text-sm font-medium leading-5">{step.title}</span>
        <Badge variant="outline" className="h-5 px-1.5 font-mono text-2xs font-normal text-muted-foreground">
          {step.actionType}
        </Badge>
        <span className="inline-flex items-center gap-1.5 text-2xs text-muted-foreground">
          <span className={cn('size-1.5 rounded-full', DOT_CLASS[RISK_TONE[risk]])} />
          {RISK_LABEL[risk]}
        </span>
        <span className="text-2xs text-muted-foreground">
          via <span className="font-mono">{step.adapter}</span>
        </span>
      </div>
      {step.description ? <p className="text-xs text-muted-foreground">{step.description}</p> : null}
      <dl className="flex flex-col gap-1 text-2xs text-muted-foreground">
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
        {step.verification ? (
          <div className="flex gap-1.5">
            <dt className="shrink-0">Verified by</dt>
            <dd>{step.verification}</dd>
          </div>
        ) : null}
      </dl>
    </li>
  )
}
