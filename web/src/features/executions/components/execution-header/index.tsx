import { XIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { StatusBadge } from '@/components/shared/data/status-badge'
import { PageHeader } from '@/components/shared/page'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import type { components } from '@/lib/api/schema'

import { EXECUTION_STATUS, adapterLabel, canCancel, canStart } from '../../lib/labels'
import { stepProgress } from '../../lib/progress'
import { CancelDialog } from './cancel-dialog'
import { StartDialog } from './start-dialog'

interface ExecutionHeaderProps {
  execution: components['schemas']['Execution']
}

export function ExecutionHeader({ execution }: ExecutionHeaderProps) {
  const status = EXECUTION_STATUS[execution.status]
  const progress = stepProgress(execution.steps)

  return (
    <PageHeader
      eyebrow={
        <>
          <span className="font-mono text-xs tabular-nums text-muted-foreground">#{execution.number}</span>
          <StatusBadge tone={status.tone} live={status.live}>
            {status.label}
          </StatusBadge>
          <span className="text-xs tabular-nums text-muted-foreground">
            {progress.succeeded}/{progress.total} steps
          </span>
        </>
      }
      title={execution.title}
      description={
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
          <Link to={routes.project(execution.projectId)} className="text-foreground hover:underline">
            {execution.projectName || 'Project'}
          </Link>
          <span className="tabular-nums">revision {execution.revision}</span>
          <Badge variant="outline" className="h-5 font-normal text-muted-foreground">
            {adapterLabel(execution.adapter)}
          </Badge>
          <span>
            as <span className="font-mono">{execution.identityId}</span>
          </span>
          <span>
            updated <RelativeTime value={execution.updatedAt} className="font-mono tabular-nums" />
          </span>
        </span>
      }
      actions={
        <>
          {canCancel(execution.status) && (
            <CancelDialog executionId={execution.id} executionNumber={execution.number}>
              <Button variant="outline">
                <XIcon data-icon="inline-start" />
                Cancel
              </Button>
            </CancelDialog>
          )}
          {canStart(execution.status) && <StartDialog execution={execution} />}
        </>
      }
    />
  )
}
