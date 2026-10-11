import { GaugeIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { StatusBadge } from '@/components/shared/data/status-badge'
import { EmptyState } from '@/components/shared/feedback'
import type { MaturityScore } from '@/features/projects/api/queries'
import { stageLabel } from '@/features/projects/lib/maturity'
import { testIds } from '@/test-ids'

import { AxisRow } from './axis-row'

interface DeliveryMaturityProps {
  maturity: MaturityScore
  /** Pattern id → display name, once the catalog has loaded. */
  patternNames?: ReadonlyMap<string, string>
  projectId: string
}

/**
 * The overall score as the hero number, then one thin bar per axis with
 * the findings that lowered it and the patterns that would raise it.
 */
export function DeliveryMaturity({ maturity, patternNames, projectId }: DeliveryMaturityProps) {
  if (maturity.axes.length === 0) {
    return (
      <EmptyState
        icon={GaugeIcon}
        title="Not scored yet"
        description="Maturity is computed from detected facts after a scan succeeds."
        className="py-8"
      />
    )
  }

  const stage = stageLabel(maturity.stage, maturity.overall)

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-end gap-x-4 gap-y-2">
        <div className="flex items-baseline gap-1">
          <span
            data-testid={testIds.understanding.maturityScore}
            className="text-3xl leading-none font-semibold tracking-tight tabular-nums text-brand"
          >
            {maturity.overall}
          </span>
          <span className="text-sm text-muted-foreground">/ 100</span>
        </div>
        <StatusBadge tone={stage.tone} className="mb-0.5">
          {stage.label}
        </StatusBadge>
        <span className="mb-0.5 font-mono text-xs text-muted-foreground">
          computed <RelativeTime value={maturity.computedAt} />
        </span>
      </div>

      <ul className="flex flex-col">
        {maturity.axes.map((axis) => (
          <AxisRow key={axis.key} axis={axis} patternNames={patternNames} projectId={projectId} />
        ))}
      </ul>
    </div>
  )
}

export { DeliveryMaturitySkeleton } from './maturity-skeleton'
