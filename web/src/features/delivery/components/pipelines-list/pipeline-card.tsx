import { TechChips } from '@/components/shared/data/tech-chip'
import { Badge } from '@/components/ui/badge'
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

import { providerLabel } from '../../lib/labels'
import type { Pipeline } from '../../lib/types'
import { JobDag } from './job-dag'

interface PipelineCardProps {
  pipeline: Pipeline
}

/** One workflow definition: name, path, triggers and its job graph. */
export function PipelineCard({ pipeline }: PipelineCardProps) {
  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle className="truncate">{pipeline.name || pipeline.path}</CardTitle>
        <CardDescription className="truncate font-mono text-2xs">{pipeline.path}</CardDescription>
        <CardAction>
          <Badge variant="secondary" className="h-5 px-1.5 text-2xs font-normal">
            {providerLabel(pipeline.provider)}
          </Badge>
        </CardAction>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-1.5 text-2xs text-muted-foreground">
          <span className="shrink-0">Triggers</span>
          {pipeline.triggers.length > 0 ? <TechChips items={pipeline.triggers} max={8} /> : <span>none declared</span>}
        </div>
        <JobDag jobs={pipeline.jobs} />
      </CardContent>
    </Card>
  )
}
