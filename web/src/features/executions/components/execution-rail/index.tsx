import { ListChecksIcon } from 'lucide-react'

import { PanelEmpty } from '@/components/system/panel'
import { Rail, RailStep, type RailStatus } from '@/components/system/rail'
import { Tag } from '@/components/system/tag'
import type { components } from '@/lib/api/schema'

import { STEP_STATUS, isPausedOnFailure } from '../../lib/labels'
import { FailedStepPanel } from './failed-step-panel'
import { InlineApproval } from './inline-approval'
import { StepRow } from './step-row'
import { useLatestLogByStep } from './use-latest-log'

export { ExecutionRailSkeleton } from './skeleton'

type StepStatus = components['schemas']['StepStatus']

interface ExecutionRailProps {
  execution: components['schemas']['Execution']
}

const RAIL_STATUS: Record<StepStatus, RailStatus> = {
  unspecified: 'pending',
  pending: 'pending',
  ready: 'ready',
  waiting_approval: 'waiting',
  running: 'running',
  verifying: 'running',
  succeeded: 'done',
  failed: 'failed',
  blocked: 'blocked',
  skipped: 'skipped',
  cancelled: 'skipped',
}

/**
 * The signature surface: steps in plan order on the shared rail. The last
 * checkpoint is tagged, a paused failure is expanded in place, and steps
 * waiting for approval carry their decision inline.
 */
export function ExecutionRail({ execution }: ExecutionRailProps) {
  const steps = [...execution.steps].sort((a, b) => a.position - b.position)
  const failure = isPausedOnFailure(execution.status) ? execution.failure : null
  const latestLog = useLatestLogByStep(execution.id)

  if (steps.length === 0) {
    return <PanelEmpty icon={ListChecksIcon} title="This plan has no steps" description="Nothing to run." />
  }

  return (
    <Rail aria-label="Execution steps">
      {steps.map((step) => {
        const isFailed = failure?.stepKey === step.key
        const isCheckpoint = execution.lastCheckpoint === step.key
        const awaitingDecision = step.status === 'waiting_approval' && Boolean(step.approvalId)
        const running = step.status === 'running' || step.status === 'verifying'
        const dimmed = step.status === 'blocked' || step.status === 'skipped' || step.status === 'cancelled'

        return (
          <RailStep
            key={step.id}
            status={RAIL_STATUS[step.status]}
            statusLabel={STEP_STATUS[step.status].label}
            title={<StepRow step={step} emphasized={isFailed} liveLine={latestLog[step.key]} />}
            emphasis={isFailed ? 'failed' : running ? 'running' : awaitingDecision ? 'waiting' : 'none'}
            dimmed={dimmed}
            badge={
              isCheckpoint ? (
                <Tag tone="success" className="h-4 bg-background px-1 text-[10px]">
                  checkpoint
                </Tag>
              ) : undefined
            }
          >
            {awaitingDecision && step.approvalId && <InlineApproval approvalId={step.approvalId} stepTitle={step.title} />}
            {isFailed && failure && <FailedStepPanel executionId={execution.id} executionNumber={execution.number} failure={failure} />}
          </RailStep>
        )
      })}
    </Rail>
  )
}
