import { StatusBadge } from '@/components/shared/data'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { POLICY_DECISION, STEP_STATUS, formatDuration } from '../../lib/labels'
import { ActionTypeChip } from '../action-type-chip'
import { RiskDot } from '../risk-dot'

interface StepRowProps {
  step: components['schemas']['Step']
  /** The failed step is the one the operator is looking at; it gets more weight. */
  emphasized?: boolean
}

export function StepRow({ step, emphasized = false }: StepRowProps) {
  const status = STEP_STATUS[step.status]
  const duration = formatDuration(step.startedAt, step.finishedAt)
  const dimmed = step.status === 'blocked' || step.status === 'skipped' || step.status === 'cancelled'

  return (
    <div className={cn('flex min-w-0 flex-col gap-1', dimmed && 'opacity-70')}>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <span className={cn('text-sm', emphasized ? 'font-semibold' : 'font-medium')}>{step.title}</span>
        <ActionTypeChip>{step.actionType}</ActionTypeChip>
        <RiskDot risk={step.risk} />
        <StatusBadge tone={status.tone} live={status.live} className="ml-auto">
          {status.label}
        </StatusBadge>
      </div>
      {step.description && step.description !== step.title && (
        <p className="text-xs text-muted-foreground">{step.description}</p>
      )}
      <dl className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
        <Meta label="policy">{POLICY_DECISION[step.policyDecision]}</Meta>
        {step.requiredPermissions.length > 0 && (
          <Meta label="needs">
            <span className="font-mono">{step.requiredPermissions.join(' ')}</span>
          </Meta>
        )}
        {step.dependsOn.length > 0 && (
          <Meta label="after">
            <span className="font-mono">{step.dependsOn.join(', ')}</span>
          </Meta>
        )}
        {step.attempts > 0 && (
          <Meta label="attempts">
            <span className="tabular-nums">{step.attempts}</span>
          </Meta>
        )}
        {duration && (
          <Meta label="took">
            <span className="font-mono tabular-nums">{duration}</span>
          </Meta>
        )}
      </dl>
    </div>
  )
}

function Meta({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="inline-flex items-baseline gap-1">
      <dt className="text-muted-foreground/70">{label}</dt>
      <dd className="text-foreground/80">{children}</dd>
    </div>
  )
}
