import { Stat } from '@/components/shared/data'

import type { OverviewMetrics } from '../../lib/metrics'
import { HeroMetric } from './hero-metric'

export { MetricStripSkeleton } from './skeleton'

/** One hero (accent) and three neutral supports in a single compact row. */
export function MetricStrip({ metrics }: { metrics: OverviewMetrics }) {
  return (
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-[minmax(0,1.6fr)_repeat(3,minmax(0,1fr))]">
      <HeroMetric waiting={metrics.waiting} />
      <Stat
        label="Projects ready"
        value={metrics.projectsReady.toLocaleString()}
        hint={`of ${metrics.projectsTotal.toLocaleString()} connected`}
      />
      <Stat
        label="Executions active"
        value={metrics.executionsActive.toLocaleString()}
        hint="running or waiting for approval"
      />
      <Stat label="Proposals open" value={metrics.proposalsOpen.toLocaleString()} hint="proposed or negotiating" />
    </div>
  )
}
