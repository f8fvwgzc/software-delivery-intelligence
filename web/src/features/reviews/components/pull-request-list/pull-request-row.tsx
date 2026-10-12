import { ExternalLinkIcon, ScanSearchIcon } from 'lucide-react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { Row } from '@/components/system/row'
import { Tag } from '@/components/system/tag'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { useStartReview } from '../../api/mutations'
import type { PullRequestSummary } from '../../api/types'
import { prState, shortSha } from '../../lib/labels'

export function PullRequestRow({ pullRequest }: { pullRequest: PullRequestSummary }) {
  const navigate = useNavigate()
  const { currentWorkspace } = useAuth()
  // One mutation per row, so two rows can start reviews independently.
  const start = useStartReview(pullRequest.projectId)
  const state = prState(pullRequest.state)
  const canStart = hasRole(currentWorkspace.myRole, 'member') && (pullRequest.state === 'open' || pullRequest.state === 'draft')

  const review = () => {
    start.mutate(pullRequest.number, {
      onSuccess: (next) => {
        toast.success(`Review of #${pullRequest.number} started`)
        navigate(routes.review(next.id))
      },
    })
  }

  return (
    <Row
      leading={
        <Avatar size="sm" className="size-4" title={pullRequest.author}>
          <AvatarImage src={pullRequest.authorAvatarUrl || undefined} alt="" />
          <AvatarFallback className="text-[9px]">{pullRequest.author.slice(0, 2).toUpperCase()}</AvatarFallback>
        </Avatar>
      }
      title={
        <span className="inline-flex min-w-0 items-baseline gap-1.5">
          <span className="truncate">{pullRequest.title}</span>
          <a
            href={pullRequest.htmlUrl}
            target="_blank"
            rel="noreferrer"
            className="inline-flex shrink-0 items-center text-muted-foreground hover:text-foreground"
            aria-label={`Open #${pullRequest.number} on GitHub`}
          >
            <ExternalLinkIcon className="size-3" />
          </a>
        </span>
      }
      meta={
        <>
          <span className="font-mono tabular-nums">#{pullRequest.number}</span>
          <span>{pullRequest.author}</span>
          <span className="font-mono">
            {pullRequest.headRef} → {pullRequest.baseRef}
          </span>
          <span className="font-mono">{shortSha(pullRequest.headSha)}</span>
          <span className="tabular-nums">
            {pullRequest.changedFiles} {pullRequest.changedFiles === 1 ? 'file' : 'files'}
          </span>
          <span>
            updated <RelativeTime value={pullRequest.updatedAt} />
          </span>
        </>
      }
      trailing={
        <>
          <span className="hidden font-mono tabular-nums sm:inline">
            <span className="text-success">+{pullRequest.additions}</span>{' '}
            <span className="text-destructive">−{pullRequest.deletions}</span>
          </span>
          <Tag tone={state.tone}>{state.label}</Tag>
          {pullRequest.latestReviewId ? (
            <Link to={routes.review(pullRequest.latestReviewId)} className="rounded-[4px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
              <Tag variant="outline">Reviewed</Tag>
            </Link>
          ) : null}
          {canStart ? (
            <Button size="xs" variant="outline" onClick={review} loading={start.isPending}>
              <ScanSearchIcon data-icon="inline-start" />
              Review
            </Button>
          ) : null}
        </>
      }
    />
  )
}
