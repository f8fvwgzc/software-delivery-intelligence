import { ChevronRightIcon, ExternalLinkIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data'
import { Button } from '@/components/ui/button'
import { TableCell, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

import { firstLine, formatDuration, shortSha } from '../../lib/format'
import { RUN_STATUS } from '../../lib/labels'
import type { PipelineRun } from '../../lib/types'
import { StatusDot } from '../status-dot'
import { FailureDetails } from './failure-details'
import { handleRowKeyDown } from './row-keyboard'

export const RUN_COLUMNS = 10

interface RunRowProps {
  run: PipelineRun
  expanded: boolean
  onToggle: (id: string) => void
}

/** One run, plus its failure details as a second row when expanded. */
export function RunRow({ run, expanded, onToggle }: RunRowProps) {
  const status = RUN_STATUS[run.status]
  const failure = run.failure ?? null
  const toggle = failure ? () => onToggle(run.id) : undefined

  return (
    <>
      <TableRow
        tabIndex={0}
        onKeyDown={(event) => handleRowKeyDown(event, toggle)}
        className="focus-visible:bg-muted/60 focus-visible:outline-none"
      >
        <TableCell className="w-8 pr-0">
          {toggle ? (
            <Button
              variant="ghost"
              size="icon-xs"
              aria-expanded={expanded}
              aria-label={expanded ? 'Hide failure details' : 'Show failure details'}
              onClick={toggle}
            >
              <ChevronRightIcon
                className={cn('transition-transform duration-150 ease-out motion-reduce:transition-none', expanded && 'rotate-90')}
              />
            </Button>
          ) : null}
        </TableCell>
        <TableCell>
          <StatusDot tone={status.tone} label={status.label} live={status.live} />
        </TableCell>
        <TableCell className="font-mono text-xs tabular-nums text-muted-foreground">#{run.runNumber}</TableCell>
        <TableCell className="max-w-48 truncate font-medium">{run.pipelineName}</TableCell>
        <TableCell className="max-w-40 truncate font-mono text-xs">{run.branch}</TableCell>
        <TableCell className="max-w-72">
          <div className="flex items-baseline gap-2">
            <span className="shrink-0 font-mono text-xs">{shortSha(run.commitSha)}</span>
            <span className="truncate text-muted-foreground">{firstLine(run.commitMessage)}</span>
          </div>
        </TableCell>
        <TableCell className="max-w-32 truncate">{run.actor}</TableCell>
        <TableCell className="font-mono text-xs">
          <RelativeTime value={run.startedAt} />
        </TableCell>
        <TableCell className="text-right font-mono text-xs tabular-nums">{formatDuration(run.durationSeconds)}</TableCell>
        <TableCell className="w-8 pl-0 text-right">
          {run.htmlUrl ? (
            <Button asChild variant="ghost" size="icon-xs" aria-label="Open run at the provider">
              <a href={run.htmlUrl} target="_blank" rel="noreferrer">
                <ExternalLinkIcon />
              </a>
            </Button>
          ) : null}
        </TableCell>
      </TableRow>
      {expanded && failure ? (
        <TableRow className="bg-muted/30 hover:bg-muted/30">
          <TableCell colSpan={RUN_COLUMNS} className="p-0 whitespace-normal">
            <FailureDetails failure={failure} />
          </TableCell>
        </TableRow>
      ) : null}
    </>
  )
}
