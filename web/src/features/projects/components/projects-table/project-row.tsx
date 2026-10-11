import { useQueryClient } from '@tanstack/react-query'
import { LockIcon, Trash2Icon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { TechChips } from '@/components/shared/data/tech-chip'
import { Button } from '@/components/ui/button'
import { TableCell, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

import { projectQueries, type Project } from '../../api/queries'
import { DeleteProjectDialog } from '../delete-project-dialog'
import { MaturityMeter } from '../maturity-meter'
import { ModeSwitch } from '../mode-switch'
import { ProjectStatus } from '../project-status'
import { RescanButton } from '../rescan-button'
import { COLUMN_CLASS } from './columns'

interface ProjectRowProps {
  project: Project
}

export function ProjectRow({ project }: ProjectRowProps) {
  const queryClient = useQueryClient()

  // The detail page opens instantly when its data is already in the cache.
  const prefetch = () => {
    queryClient.prefetchQuery({ ...projectQueries.detail(project.id), staleTime: 60_000 })
  }

  return (
    <TableRow className="group/row">
      <TableCell className={COLUMN_CLASS.name}>
        <Link
          to={routes.project(project.id)}
          onMouseEnter={prefetch}
          onFocus={prefetch}
          className="inline-flex items-center gap-1.5 rounded-sm font-mono text-sm outline-none hover:underline hover:underline-offset-4 focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          <span className="truncate">{project.repoFullName}</span>
          {project.isPrivate ? (
            <LockIcon className="size-3 shrink-0 text-muted-foreground" aria-label="Private repository" />
          ) : null}
        </Link>
      </TableCell>

      <TableCell className={COLUMN_CLASS.status}>
        <ProjectStatus status={project.status} />
      </TableCell>

      <TableCell className={COLUMN_CLASS.mode}>
        <ModeSwitch project={project} size="sm" />
      </TableCell>

      <TableCell className={cn(COLUMN_CLASS.stack, 'max-w-64 overflow-hidden')}>
        {project.stack.length > 0 ? (
          <TechChips items={project.stack} max={4} className="flex-nowrap" />
        ) : (
          <span className="text-xs text-muted-foreground">Not scanned yet</span>
        )}
      </TableCell>

      <TableCell className={COLUMN_CLASS.maturity}>
        <MaturityMeter score={project.maturityScore} unscored={!project.lastScannedAt} />
      </TableCell>

      <TableCell className={cn(COLUMN_CLASS.questions, project.openQuestions === 0 && 'text-muted-foreground')}>
        {project.openQuestions}
      </TableCell>

      <TableCell className={COLUMN_CLASS.scanned}>
        <RelativeTime value={project.lastScannedAt} fallback="Never" className="font-mono text-xs text-muted-foreground" />
      </TableCell>

      <TableCell className={COLUMN_CLASS.actions}>
        <div className="flex items-center justify-end gap-1 text-muted-foreground group-hover/row:text-foreground">
          <RescanButton project={project} iconOnly variant="ghost" />
          <DeleteProjectDialog
            project={project}
            tooltip="Delete"
            trigger={
              <Button variant="ghost" size="icon-sm" aria-label={`Delete ${project.name}`}>
                <Trash2Icon />
              </Button>
            }
          />
        </div>
      </TableCell>
    </TableRow>
  )
}
