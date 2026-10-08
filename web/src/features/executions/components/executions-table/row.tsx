import { useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime, StatusBadge } from '@/components/shared/data'
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
    <TableRow
      tabIndex={0}
      className="cursor-pointer outline-none focus-visible:bg-muted/60 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50"
      onClick={() => navigate(href)}
      onKeyDown={(event) => {
        if (event.key === 'Enter') navigate(href)
      }}
      onMouseEnter={prefetch}
      onFocus={prefetch}
    >
      <TableCell className="w-16 font-mono text-xs tabular-nums text-muted-foreground">#{execution.number}</TableCell>
      <TableCell className="max-w-72">
        <Link
          to={href}
          className="block truncate font-medium outline-none hover:underline focus-visible:underline"
          onClick={(event) => event.stopPropagation()}
          tabIndex={-1}
        >
          {execution.title}
        </Link>
      </TableCell>
      {showProject && (
        <TableCell className="max-w-48">
          <Link
            to={routes.project(execution.projectId)}
            className="block truncate text-muted-foreground hover:text-foreground hover:underline"
            onClick={(event) => event.stopPropagation()}
            tabIndex={-1}
          >
            {execution.projectName}
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
