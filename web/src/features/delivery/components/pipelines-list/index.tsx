import type { Pipeline } from '../../lib/types'
import { PipelineCard } from './pipeline-card'

interface PipelinesListProps {
  pipelines: Pipeline[]
}

/** Workflow definitions, one compact block each. */
export function PipelinesList({ pipelines }: PipelinesListProps) {
  return (
    <div className="flex flex-col gap-3">
      {pipelines.map((pipeline) => (
        <PipelineCard key={pipeline.id} pipeline={pipeline} />
      ))}
    </div>
  )
}

export { PipelinesSkeleton } from './skeleton'
