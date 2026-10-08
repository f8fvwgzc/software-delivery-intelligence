import { FolderGitIcon, SearchXIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

import type { GithubRepo } from '../../api/types'
import { RepoRow } from './repo-row'
import { handleRowNavigation } from './row-navigation'

export { RepoTableSkeleton } from './skeleton'

const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

interface RepoTableProps {
  repos: GithubRepo[]
  /** The filter these rows answer to; drives the no-match state. */
  query: string
  /** The typed filter is ahead of `query`; rows are shown dimmed meanwhile. */
  stale: boolean
  /** `fullName` of the repository being connected, if any. */
  connectingFullName: string | null
  onConnect: (repo: GithubRepo) => void
  onClearQuery: () => void
}

/** Dense repository list: visibility dot, mono name, language, last push, one action. */
export function RepoTable({ repos, query, stale, connectingFullName, onConnect, onClearQuery }: RepoTableProps) {
  if (repos.length === 0 && query) {
    return (
      <EmptyState
        icon={SearchXIcon}
        title={`No repositories match “${query}”`}
        description="The filter is applied to the repository name only. Try a shorter fragment."
        action={
          <Button variant="outline" size="sm" onClick={onClearQuery}>
            Clear filter
          </Button>
        }
      />
    )
  }

  if (repos.length === 0) {
    return (
      <EmptyState
        icon={FolderGitIcon}
        title="The token can see no repositories"
        description="Grant it the repo scope, or sign in with an account that owns or collaborates on repositories."
      />
    )
  }

  return (
    <div className={cn('overflow-hidden rounded-lg ring-1 ring-border transition-opacity duration-150', stale && 'opacity-60')}>
      <Table onKeyDown={handleRowNavigation}>
        <TableHeader>
          <TableRow className="bg-muted/30 hover:bg-muted/30">
            <TableHead className={HEAD_CLASS}>Repository</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden md:table-cell`}>Language</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden sm:table-cell`}>Pushed</TableHead>
            <TableHead className={`${HEAD_CLASS} text-right`}>
              <span className="sr-only">Action</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {repos.map((repo) => (
            <RepoRow
              key={repo.id}
              repo={repo}
              connecting={connectingFullName === repo.fullName}
              disabled={connectingFullName !== null && connectingFullName !== repo.fullName}
              onConnect={onConnect}
            />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
