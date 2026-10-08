import { ExternalLinkIcon, LockIcon, Trash2Icon } from 'lucide-react'
import { useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { StatusBadge } from '@/components/shared/data/status-badge'
import { PageHeader } from '@/components/shared/page'
import { Button } from '@/components/ui/button'

import type { Project } from '../../api/queries'
import { PROJECT_STATUS } from '../../lib/labels'
import { DeleteProjectDialog } from '../delete-project-dialog'
import { ModeSwitch } from '../mode-switch'
import { RescanButton } from '../rescan-button'

/** Title band of every project page: what it is, where it stands, what can be done to it. */
export function ProjectHeader({ project }: { project: Project }) {
  const navigate = useNavigate()
  const status = PROJECT_STATUS[project.status]

  return (
    <PageHeader
      eyebrow={
        <>
          <StatusBadge tone={status.tone} live={status.live}>
            {status.label}
          </StatusBadge>
          <span className="font-mono text-xs text-muted-foreground">
            {project.defaultBranch}
            {project.lastScannedAt ? (
              <>
                {' · scanned '}
                <RelativeTime value={project.lastScannedAt} />
              </>
            ) : null}
          </span>
        </>
      }
      title={
        <span className="inline-flex items-center gap-2">
          <span className="font-mono">{project.repoFullName}</span>
          {project.isPrivate ? (
            <LockIcon className="size-3.5 shrink-0 text-muted-foreground" aria-label="Private repository" />
          ) : null}
          <a
            href={project.repoUrl}
            target="_blank"
            rel="noreferrer"
            className="rounded-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
            aria-label="Open on GitHub"
          >
            <ExternalLinkIcon className="size-3.5" />
          </a>
        </span>
      }
      description={project.tldr || project.description || 'No summary yet. The first scan writes one.'}
      actions={
        <>
          <ModeSwitch project={project} className="mr-1" />
          <RescanButton project={project} />
          <DeleteProjectDialog
            project={project}
            onDeleted={() => navigate(routes.projects())}
            tooltip="Delete project"
            trigger={
              <Button variant="ghost" size="icon" aria-label={`Delete ${project.name}`}>
                <Trash2Icon />
              </Button>
            }
          />
        </>
      }
    />
  )
}

export { ProjectHeaderSkeleton } from './project-header-skeleton'
