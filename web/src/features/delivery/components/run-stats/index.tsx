import { CircleXIcon, TimerIcon } from 'lucide-react'

import { ScoreBar, Stat } from '@/components/shared/data'
import { Card } from '@/components/ui/card'

import { formatDuration, percent } from '../../lib/format'
import type { PipelineStats } from '../../lib/types'
import { count } from '@/lib/intl'

interface RunStatsProps {
  stats: PipelineStats | null | undefined
}

/** Hero: success rate. Supports: average duration and failed count. */
export function RunStats({ stats }: RunStatsProps) {
  if (!stats) {
    return <p className="text-xs text-muted-foreground">No completed runs to summarise yet.</p>
  }

  const completed = stats.succeeded + stats.failed

  return (
    <div className="grid gap-3 sm:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)]">
      <Card size="sm" className="gap-2 bg-brand/[0.04] px-3 py-3 ring-brand/20">
        <div className="text-xs text-muted-foreground">Success rate</div>
        <div className="text-2xl font-semibold tabular-nums leading-none tracking-tight">{percent(stats.successRate)}%</div>
        <ScoreBar value={stats.successRate} showValue={false} />
        <div className="text-2xs tabular-nums text-muted-foreground">
          {stats.succeeded} of {completed} completed runs succeeded
        </div>
      </Card>
      <Stat
        label="Average duration"
        value={<span className="font-mono">{formatDuration(stats.avgDurationSeconds)}</span>}
        hint={`over ${count(stats.total, 'run', 'runs')}`}
        icon={TimerIcon}
      />
      <Stat
        label="Failed"
        value={stats.failed}
        tone={stats.failed > 0 ? 'destructive' : 'default'}
        hint={stats.failed > 0 ? 'expand a row for the cause' : 'nothing to fix'}
        icon={CircleXIcon}
      />
    </div>
  )
}
