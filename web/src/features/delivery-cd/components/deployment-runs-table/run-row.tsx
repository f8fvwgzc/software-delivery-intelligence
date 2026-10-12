import { ArrowRightIcon, ExternalLinkIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { Tag } from '@/components/system/tag'
import { TableCell, TableRow } from '@/components/ui/table'

import type { DeploymentRun } from '../../api/types'
import { deploymentKind, formatDuration, hostOf, isRunOpen, needsAttention, runStatus, safeHttpUrl } from '../../lib/labels'

const LINK_CLASS =
  'inline-flex items-center gap-1 rounded-sm text-muted-foreground outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50'

/** One promotion or rollback: where, what, how it went, who started it, and the way into its execution. */
export function RunRow({ run }: { run: DeploymentRun }) {
  const status = runStatus(run)
  const kind = deploymentKind(run.kind)
  const external = safeHttpUrl(run.externalUrl)
  const open = isRunOpen(run)
  const statusTag = (
    <Tag tone={status.tone} dot live={status.live}>
      {status.label}
    </Tag>
  )

  return (
    <TableRow className="focus-within:bg-muted/50">
      <TableCell className="max-w-40 px-3 py-2">
        <span className="block truncate font-medium" title={run.environmentName}>
          {run.environmentName}
        </span>
      </TableCell>
      <TableCell className="max-w-40 px-3 py-2">
        <span className="block truncate font-mono text-xs" title={run.releaseVersion}>
          {run.releaseVersion}
        </span>
      </TableCell>
      <TableCell className="px-3 py-2">
        <Tag tone={kind.tone} variant={run.kind === 'rollback' ? 'tint' : 'outline'}>
          {kind.label}
        </Tag>
      </TableCell>
      <TableCell className="px-3 py-2">
        {needsAttention(run) ? (
          // Paused, not over: the status itself leads to the execution where it is resolved.
          <Link
            to={routes.execution(run.executionId)}
            className="inline-flex rounded-[4px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
            aria-label={`${status.label}: resolve the ${kind.label.toLowerCase()} of ${run.releaseVersion} to ${run.environmentName} on its execution`}
          >
            {statusTag}
          </Link>
        ) : (
          statusTag
        )}
      </TableCell>
      <TableCell className="hidden max-w-36 truncate px-3 py-2 text-xs text-muted-foreground md:table-cell">{run.startedBy || 'system'}</TableCell>
      <TableCell className="hidden px-3 py-2 font-mono text-xs tabular-nums text-muted-foreground sm:table-cell">
        {open || !run.durationSeconds ? '—' : formatDuration(run.durationSeconds)}
      </TableCell>
      <TableCell className="hidden px-3 py-2 text-xs text-muted-foreground lg:table-cell">
        <RelativeTime value={run.createdAt} className="font-mono tabular-nums" />
      </TableCell>
      <TableCell className="px-3 py-2">
        <span className="flex items-center justify-end gap-3 text-xs">
          {external ? (
            <a href={external} target="_blank" rel="noreferrer" className={LINK_CLASS} aria-label={`Open the run at ${hostOf(external)}`}>
              <span className="hidden xl:inline">{hostOf(external)}</span>
              <ExternalLinkIcon className="size-3" aria-hidden />
            </a>
          ) : null}
          <Link
            to={routes.execution(run.executionId)}
            className={LINK_CLASS}
            aria-label={`Execution of the ${kind.label.toLowerCase()} of ${run.releaseVersion} to ${run.environmentName}`}
          >
            Execution
            <ArrowRightIcon className="size-3" aria-hidden />
          </Link>
        </span>
      </TableCell>
    </TableRow>
  )
}
