import { useQuery } from '@tanstack/react-query'

import { Stat, StatStrip } from '@/components/system/stat'
import { Skeleton } from '@/components/ui/skeleton'
import { count } from '@/lib/intl'

import { cdQueries } from '../../api/queries'
import type { DeliveryPipeline } from '../../api/types'
import { formatLeadTime, percent, registryPath } from '../../lib/labels'
import { registryAuthLabel, registryKind } from '../../lib/registries'

const NONE = '—'

function RegistryValue({ pipeline }: { pipeline: DeliveryPipeline }) {
  if (!pipeline.registry) return NONE
  const path = registryPath(pipeline.registry)
  return (
    // Smaller than a number so a long host still fits; the line height of the other values keeps the strip level.
    <span className="block truncate font-mono text-sm leading-7 font-medium tracking-normal" title={path}>
      {path}
    </span>
  )
}

/**
 * The 30-day numbers of the board and where its images go. Reads the same
 * query as the stages below (one request); while that loads or fails the
 * strip keeps its shape, and the panel under it carries the error.
 */
export function PipelineStats({ projectId }: { projectId: string }) {
  const { data: pipeline, isPending } = useQuery(cdQueries.pipeline(projectId))
  const placeholder = isPending ? <Skeleton className="my-1 h-5 w-16" /> : NONE
  const registry = pipeline?.registry

  return (
    <StatStrip>
      <Stat
        label="Success rate"
        // A paused run is not finished: with nothing finished there is no rate yet, which is not the same as 0%.
        value={pipeline ? (pipeline.finishedRuns30d > 0 ? `${percent(pipeline.successRate)}%` : NONE) : placeholder}
        delta={
          pipeline && pipeline.finishedRuns30d > 0
            ? `of ${count(pipeline.finishedRuns30d, 'finished run', 'finished runs')}, 30 days`
            : 'no run has finished in 30 days'
        }
      />
      <Stat
        label="Deployment runs"
        value={pipeline ? pipeline.deployments30d.toLocaleString() : placeholder}
        delta={pipeline ? `started in 30 days, ${count(pipeline.stages.length, 'environment', 'environments')}` : 'started in 30 days'}
      />
      <Stat
        label="Median lead time"
        value={pipeline ? pipeline.leadTimeSeconds > 0 ? <span className="font-mono">{formatLeadTime(pipeline.leadTimeSeconds)}</span> : NONE : placeholder}
        delta="release to last stage"
      />
      <Stat
        label="Default registry"
        value={pipeline ? <RegistryValue pipeline={pipeline} /> : placeholder}
        delta={registry ? `${registryKind(registry.kind).label}, ${registryAuthLabel(registry.auth).toLowerCase()}` : 'where images are pushed'}
      />
    </StatStrip>
  )
}
