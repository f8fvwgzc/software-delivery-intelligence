import { ArrowRightIcon, PlugIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data'
import { TechChip } from '@/components/shared/data/tech-chip'
import { Button } from '@/components/ui/button'
import { TableCell, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

import type { GithubRepo } from '../../api/types'
import { DOT_CLASS, visibilityLabel, visibilityTone } from '../../lib/labels'

interface RepoRowProps {
  repo: GithubRepo
  /** This row's connect request is in flight. */
  connecting: boolean
  /** Another row is connecting; keep this one quiet. */
  disabled: boolean
  onConnect: (repo: GithubRepo) => void
}

export function RepoRow({ repo, connecting, disabled, onConnect }: RepoRowProps) {
  const flags = [repo.archived ? 'archived' : null, repo.fork ? 'fork' : null].filter(Boolean).join(' · ')

  return (
    <TableRow className="focus-within:bg-muted/50">
      <TableCell className="max-w-0 px-3 py-2">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="flex items-center gap-2">
            <span
              aria-hidden
              title={visibilityLabel(repo)}
              className={cn('size-1.5 shrink-0 rounded-full', DOT_CLASS[visibilityTone(repo)])}
            />
            <span className="sr-only">{visibilityLabel(repo)}</span>
            <span className="truncate font-mono text-xs">
              <span className="text-muted-foreground">{repo.owner}/</span>
              <span className="text-foreground">{repo.name}</span>
            </span>
            {flags ? <span className="shrink-0 text-2xs text-muted-foreground">{flags}</span> : null}
          </span>
          {repo.description ? (
            <span className="truncate pl-3.5 text-xs text-muted-foreground" title={repo.description}>
              {repo.description}
            </span>
          ) : null}
        </div>
      </TableCell>
      <TableCell className="hidden px-3 py-2 md:table-cell">
        {repo.language ? <TechChip>{repo.language}</TechChip> : <span className="text-xs text-muted-foreground">—</span>}
      </TableCell>
      <TableCell className="hidden px-3 py-2 font-mono text-xs text-muted-foreground sm:table-cell">
        <RelativeTime value={repo.pushedAt} fallback="never" />
      </TableCell>
      <TableCell className="px-3 py-1.5 text-right">
        {repo.projectId ? (
          <Button asChild variant="ghost" size="sm" data-row-link>
            <Link to={routes.project(repo.projectId)}>
              Open
              <ArrowRightIcon data-icon="inline-end" />
            </Link>
          </Button>
        ) : (
          <Button
            variant="outline"
            size="sm"
            data-row-link
            loading={connecting}
            disabled={disabled || repo.archived}
            title={repo.archived ? 'Archived repositories cannot be connected' : undefined}
            onClick={() => onConnect(repo)}
          >
            <PlugIcon data-icon="inline-start" />
            Connect
          </Button>
        )}
      </TableCell>
    </TableRow>
  )
}
