import { ListChecksIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Badge } from '@/components/ui/badge'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { isPausedOnFailure } from '../../lib/labels'
import { FailedStepPanel } from './failed-step-panel'
import { InlineApproval } from './inline-approval'
import { RailNode } from './rail-node'
import { StepRow } from './step-row'
import { useLatestLogByStep } from './use-latest-log'

export { ExecutionRailSkeleton } from './skeleton'

interface ExecutionRailProps {
  execution: components['schemas']['Execution']
}

/**
 * The signature surface: steps in plan order on a vertical rail. The last
 * checkpoint is tagged, a paused failure is expanded in place, and steps
 * waiting for approval carry their decision inline.
 */
export function ExecutionRail({ execution }: ExecutionRailProps) {
  const steps = [...execution.steps].sort((a, b) => a.position - b.position)
  const failure = isPausedOnFailure(execution.status) ? execution.failure : null
  const latestLog = useLatestLogByStep(execution.id)

  if (steps.length === 0) {
    return <EmptyState icon={ListChecksIcon} title="This plan has no steps" description="Nothing to run." />
  }

  return (
    <ol className="flex flex-col" aria-label="Execution steps">
      {steps.map((step) => {
        const isFailed = failure?.stepKey === step.key
        const isCheckpoint = execution.lastCheckpoint === step.key
        const awaitingDecision = step.status === 'waiting_approval' && Boolean(step.approvalId)

        return (
          <li
            key={step.id}
            className={cn(
              'relative flex gap-3 pb-5 last:pb-0',
              'after:absolute after:top-7 after:bottom-0 after:left-[11px] after:w-px after:bg-border last:after:hidden',
            )}
          >
            <div className="flex shrink-0 flex-col items-center gap-1.5 pt-0.5">
              <RailNode status={step.status} />
              {isCheckpoint && (
                <Badge
                  variant="outline"
                  className="z-10 h-4 border-success/30 bg-background px-1 text-2xs font-normal text-success"
                >
                  checkpoint
                </Badge>
              )}
            </div>
            <div
              className={cn(
                'flex min-w-0 flex-1 flex-col gap-3 rounded-lg px-3 py-2.5 ring-1 ring-border',
                step.status === 'running' && 'ring-brand/30',
                isFailed && 'ring-destructive/30',
              )}
            >
              <StepRow step={step} emphasized={isFailed} liveLine={latestLog[step.key]} />
              {awaitingDecision && step.approvalId && (
                <InlineApproval approvalId={step.approvalId} stepTitle={step.title} />
              )}
              {isFailed && failure && (
                <FailedStepPanel executionId={execution.id} executionNumber={execution.number} failure={failure} />
              )}
            </div>
          </li>
        )
      })}
    </ol>
  )
}
