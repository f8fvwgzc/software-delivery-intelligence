import { TriangleAlertIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Tag } from '@/components/system/tag'
import { TableCell, TableRow } from '@/components/ui/table'

import type { Application } from '../../api/types'
import { isOperationRunning, operationKind, sourceTypeLabel } from '../../lib/labels'
import { RevisionLink } from '../revision-link'
import { AutomationTags, HealthTag, SyncTag } from '../status-tags'
import { AUTOMATION_CLASS, CELL_CLASS, DESTINATION_CLASS } from './columns'

interface ApplicationRowProps {
  application: Application
  /** GitHub URL of the repository it renders, for the revision link. */
  repoUrl?: string
}

/** One application: what it renders, how the cluster compares, where it deploys, the commit, and what syncs on its own. */
export function ApplicationRow({ application, repoUrl }: ApplicationRowProps) {
  const { source, operation } = application
  const running = isOperationRunning(operation)
  const condition = application.conditions[0]

  return (
    <TableRow>
      <TableCell className={`${CELL_CLASS} max-w-96`}>
        <Link
          to={routes.application(application.id)}
          className="block truncate rounded-sm text-sm font-medium outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          {application.name}
        </Link>
        <span className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
          {source.path ? (
            <span className="truncate font-mono" title={source.path}>
              {source.path}
            </span>
          ) : (
            <span className="shrink-0">repository root</span>
          )}
          <span className="shrink-0">{sourceTypeLabel(source.type)}</span>
          {source.repo ? <span className="truncate font-mono">{source.repo}</span> : null}
        </span>
        {condition ? (
          <span className="flex min-w-0 items-center gap-1 text-xs text-warning" title={condition}>
            <TriangleAlertIcon className="size-3 shrink-0" aria-hidden />
            <span className="truncate">{condition}</span>
          </span>
        ) : null}
      </TableCell>

      <TableCell className={`${CELL_CLASS} whitespace-nowrap`}>
        <span className="flex flex-col items-start gap-1">
          {running && operation ? (
            <Tag tone="info" dot live>
              {operation.kind === 'rollback' ? 'Rolling back' : `${operationKind(operation.kind)} running`}
            </Tag>
          ) : (
            <SyncTag status={application.syncStatus} />
          )}
          {application.syncStatus === 'out_of_sync' && application.outOfSyncCount > 0 && !running ? (
            <span className="text-2xs tabular-nums text-muted-foreground">
              {application.outOfSyncCount} of {application.resourceCount} differ
            </span>
          ) : null}
        </span>
      </TableCell>

      <TableCell className={`${CELL_CLASS} whitespace-nowrap`}>
        <HealthTag status={application.healthStatus} message={application.healthMessage} />
      </TableCell>

      <TableCell className={`${CELL_CLASS} ${DESTINATION_CLASS} max-w-64`}>
        <span className="block truncate text-xs" title={application.clusterName}>
          {application.clusterName}
        </span>
        {application.namespace ? (
          <span className="block truncate font-mono text-xs text-muted-foreground">{application.namespace}</span>
        ) : (
          <span className="block text-xs text-muted-foreground">cluster default</span>
        )}
      </TableCell>

      <TableCell className={`${CELL_CLASS} whitespace-nowrap`}>
        <RevisionLink revision={application.revision} repoUrl={repoUrl} fallback="Not compared" className="block text-xs" />
        {source.revision ? (
          <span className="block font-mono text-2xs text-muted-foreground">{source.revision}</span>
        ) : (
          <span className="block text-2xs text-muted-foreground">default branch</span>
        )}
      </TableCell>

      <TableCell className={`${CELL_CLASS} ${AUTOMATION_CLASS}`}>
        <AutomationTags policy={application.syncPolicy} />
      </TableCell>
    </TableRow>
  )
}
