import { Rail, RailStep, type RailStatus } from '@/components/system/rail'
import type { ScanStage, StageStatus } from '@/features/projects/api/queries'
import { testIds } from '@/test-ids'

const RAIL_STATUS: Record<StageStatus, RailStatus> = {
  unspecified: 'pending',
  pending: 'pending',
  running: 'running',
  done: 'done',
  failed: 'failed',
  skipped: 'skipped',
}

/**
 * The scan's stages on the shared step rail. Stage changes arrive live, so
 * each marker fades between its states; nothing moves.
 */
export function StageRail({ stages }: { stages: ScanStage[] }) {
  return (
    <Rail data-testid={testIds.understanding.scanRail} aria-label="Scan stages">
      {stages.map((stage) => {
        const isRunning = stage.status === 'running'
        const isPending = stage.status === 'pending' || stage.status === 'unspecified'
        return (
          <RailStep
            key={stage.key}
            status={RAIL_STATUS[stage.status]}
            compact
            dimmed={isPending}
            title={
              <span className={stage.status === 'failed' ? 'text-destructive' : undefined}>
                {stage.label}
                {isRunning ? <span className="sr-only"> (running)</span> : null}
              </span>
            }
            detail={
              stage.detail ? (
                <span className="block truncate font-mono text-2xs" title={stage.detail}>
                  {stage.detail}
                </span>
              ) : undefined
            }
          />
        )
      })}
    </Rail>
  )
}
