import { useSuspenseQuery } from '@tanstack/react-query'
import { ActivityIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { EmptyState } from '@/components/shared/feedback'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { executionQueries } from '../../api/queries'
import { ExecutionRow } from './row'

export { ExecutionsTableSkeleton } from './skeleton'

interface ExecutionsTableProps {
  /** Scope the list to one project; the project column is hidden then. */
  projectId?: string
  /** Primary action for the empty state, e.g. the "New dry run" trigger. */
  emptyAction?: ReactNode
}

/** Every execution, newest first; live while any is non-terminal. */
export function ExecutionsTable({ projectId, emptyAction }: ExecutionsTableProps) {
  const { data: executions } = useSuspenseQuery(executionQueries.list(projectId ? { projectId } : {}))
  const showProject = !projectId

  if (executions.length === 0) {
    return (
      <EmptyState
        icon={ActivityIcon}
        title="No executions yet"
        description={
          projectId
            ? 'Accept a proposal for this project or start a dry run to rehearse a plan.'
            : 'Executions appear here once a proposal is accepted or a dry run is started.'
        }
        action={emptyAction}
      />
    )
  }

  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader className="bg-muted/40">
          <TableRow className="hover:bg-transparent">
            <TableHead className="h-10 text-xs">#</TableHead>
            <TableHead className="h-10 text-xs">Title</TableHead>
            {showProject && <TableHead className="h-10 text-xs">Project</TableHead>}
            <TableHead className="h-10 text-xs">Status</TableHead>
            <TableHead className="h-10 text-xs">Progress</TableHead>
            <TableHead className="h-10 text-xs">Adapter</TableHead>
            <TableHead className="h-10 text-right text-xs">Updated</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {executions.map((execution) => (
            <ExecutionRow key={execution.id} execution={execution} showProject={showProject} />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
