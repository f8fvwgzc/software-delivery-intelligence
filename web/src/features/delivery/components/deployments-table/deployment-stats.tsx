import { ScoreBar } from '@/components/shared/data'
import { cn } from '@/lib/utils'

import type { DeploymentStats } from '../../lib/types'

interface DeploymentStatsStripProps {
  stats: DeploymentStats | null | undefined
}

/** One line of context above the table; the hero of this page is the run success rate. */
export function DeploymentStatsStrip({ stats }: DeploymentStatsStripProps) {
  if (!stats) return null

  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-1 text-xs text-muted-foreground">
      <ScoreBar value={stats.successRate} label="Success rate" className="w-64 max-w-full" />
      <span>
        <span className="font-medium tabular-nums text-foreground">{stats.total}</span> deployments
      </span>
      <span>
        <span className="tabular-nums text-foreground">{stats.succeeded}</span> succeeded
      </span>
      <span>
        <span className={cn('tabular-nums', stats.failed > 0 ? 'text-destructive' : 'text-foreground')}>{stats.failed}</span>{' '}
        failed
      </span>
    </div>
  )
}
