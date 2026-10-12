import { Stat, StatStrip } from '@/components/system/stat'

import { describeWaiting, type OverviewMetrics } from '../../lib/metrics'

export { MetricStripSkeleton } from './skeleton'

/** One brand-toned hero (what waits on the human) and three neutral supports, hairlines between. */
export function MetricStrip({ metrics }: { metrics: OverviewMetrics }) {
  return (
    <StatStrip>
      <Stat label="Waiting on you" value={metrics.waiting.total.toLocaleString()} delta={describeWaiting(metrics.waiting)} tone="brand" size="lg" />
      <Stat label="Projects ready" value={metrics.projectsReady.toLocaleString()} delta={`of ${metrics.projectsTotal.toLocaleString()} connected`} />
      <Stat label="Executions active" value={metrics.executionsActive.toLocaleString()} delta="running or waiting for approval" />
      <Stat label="Proposals open" value={metrics.proposalsOpen.toLocaleString()} delta="proposed or negotiating" />
    </StatStrip>
  )
}
