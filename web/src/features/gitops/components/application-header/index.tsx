import { useQuery } from '@tanstack/react-query'
import type { ReactNode } from 'react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { PageHeader } from '@/components/shared/page'

import { gitopsQueries } from '../../api/queries'
import type { Application } from '../../api/types'
import { destinationLabel, sourceTypeLabel } from '../../lib/labels'
import { AutomationTags, HealthTag, SyncTag } from '../status-tags'

interface ApplicationHeaderProps {
  application: Application
  actions?: ReactNode
}

/** Name, sync and health as the eyebrow, where it deploys and what it renders, and the page's actions. */
export function ApplicationHeader({ application, actions }: ApplicationHeaderProps) {
  // The project's name for the way back; loaded by the project pages already, or once here.
  const { data: project } = useQuery(gitopsQueries.project(application.projectId))
  const { source } = application

  return (
    <PageHeader
      eyebrow={
        <>
          <SyncTag status={application.syncStatus} />
          <HealthTag status={application.healthStatus} message={application.healthMessage} />
          <AutomationTags policy={application.syncPolicy} />
          <Link
            to={routes.projectApplications(application.projectId)}
            className="rounded-sm text-xs text-muted-foreground outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            {project ? `${project.name} applications` : 'Project applications'}
          </Link>
        </>
      }
      title={application.name}
      description={
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span>
            to <span className="font-mono">{destinationLabel(application.clusterName, application.namespace)}</span>
          </span>
          <span>
            from <span className="font-mono">{source.path || 'the repository root'}</span>
            {source.repo ? (
              <>
                {' '}
                of <span className="font-mono">{source.repo}</span>
              </>
            ) : null}
            {' at '}
            <span className="font-mono">{source.revision || 'the default branch'}</span>
          </span>
          <span>{sourceTypeLabel(source.type)}</span>
          <span>
            compared <RelativeTime value={application.reconciledAt} fallback="not yet" className="font-mono tabular-nums" />
          </span>
        </span>
      }
      actions={actions}
    />
  )
}
