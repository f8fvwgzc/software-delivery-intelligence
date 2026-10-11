import { useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { StatusBadge } from '@/components/shared/data/status-badge'
import { Badge } from '@/components/ui/badge'
import { TableCell, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'

import { executionQueries } from '../../api/queries'
import { EXECUTION_STATUS, adapterLabel } from '../../lib/labels'
import { stepProgress } from '../../lib/progress'
import { ProgressBar } from './progress-bar'

interface ExecutionRowProps {
  execution: components['schemas']['Execution']
  /** Hidden when the table is already scoped to one project. */
  showProject: boolean
}

export function ExecutionRow({ execution, showProject }: ExecutionRowProps) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const status = EXECUTION_STATUS[execution.status]
  const progress = stepProgress(execution.steps)
  const href = routes.execution(execution.id)

  const prefetch = () => {
    queryClient.prefetchQuery({ ...executionQueries.detail(execution.id), staleTime: 10_000 })
  }

  return (
    // The row click is a pointer convenience only; the title link is the
    // real navigation and the keyboard target, so the row itself stays out
    // of the tab order and carries no role it cannot honour.
    <TableRow className="cursor-pointer" onClick={() => navigate(href)} onMouseEnter={prefetch}>
      <TableCell className="w-16 font-mono text-xs tabular-nums text-muted-foreground">#{execution.number}</TableCell>
      <TableCell className="max-w-72">
        <Link
          to={href}
          className="block truncate rounded-sm font-medium outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
          onClick={(event) => event.stopPropagation()}
          onFocus={prefetch}
        >
          {execution.title}
        </Link>
      </TableCell>
      {showProject && (
        <TableCell className="max-w-48">
          <Link
            to={routes.project(execution.projectId)}
            className="block truncate rounded-sm text-muted-foreground outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
            onClick={(event) => event.stopPropagation()}
          >
            {execution.projectName || 'Project'}
          </Link>
        </TableCell>
      )}
      <TableCell>
        <StatusBadge tone={status.tone} live={status.live}>
          {status.label}
        </StatusBadge>
      </TableCell>
      <TableCell className="w-44">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs tabular-nums text-muted-foreground">
            {progress.succeeded}/{progress.total} steps
          </span>
          <ProgressBar progress={progress} />
        </div>
      </TableCell>
      <TableCell>
        <Badge variant="outline" className="font-normal text-muted-foreground">
          {adapterLabel(execution.adapter)}
        </Badge>
      </TableCell>
      <TableCell className="text-right text-xs text-muted-foreground">
        <RelativeTime value={execution.updatedAt} className="font-mono tabular-nums" />
      </TableCell>
    </TableRow>
  )
}
