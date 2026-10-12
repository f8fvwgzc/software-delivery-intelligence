import { ExternalLinkIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { PageHeader } from '@/components/shared/page'
import { Tag } from '@/components/system/tag'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'

import type { Review } from '../../api/types'
import { REVIEW_STATUS, actorAvatarUrl, shortSha } from '../../lib/labels'

interface ReviewHeaderProps {
  review: Review
  actions?: React.ReactNode
}

/** PR title, number, author and head sha; the review's state as the eyebrow. */
export function ReviewHeader({ review, actions }: ReviewHeaderProps) {
  const status = REVIEW_STATUS[review.status]

  return (
    <PageHeader
      eyebrow={
        <>
          <span className="font-mono text-xs tabular-nums text-muted-foreground">#{review.prNumber}</span>
          <Tag tone={status.tone} dot live={status.live}>
            {status.label}
          </Tag>
          <Link to={routes.project(review.projectId)} className="text-xs text-muted-foreground hover:text-foreground hover:underline">
            {review.projectName || 'Project'}
          </Link>
        </>
      }
      title={review.prTitle}
      description={
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="inline-flex items-center gap-1.5">
            <Avatar size="sm" className="size-4">
              <AvatarImage src={actorAvatarUrl(review.prAuthor)} alt="" />
              <AvatarFallback className="text-[9px]">{review.prAuthor.slice(0, 2).toUpperCase()}</AvatarFallback>
            </Avatar>
            {review.prAuthor}
          </span>
          <span className="font-mono">{shortSha(review.headSha)}</span>
          <span className="tabular-nums">
            {review.filesReviewed} {review.filesReviewed === 1 ? 'file' : 'files'} reviewed
          </span>
          <span>reasoner {review.reasoner || 'heuristic'}</span>
          <span>
            updated <RelativeTime value={review.updatedAt} className="font-mono tabular-nums" />
          </span>
          {review.publishedUrl ? (
            <a href={review.publishedUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-foreground hover:underline">
              Published review
              <ExternalLinkIcon className="size-3" />
            </a>
          ) : null}
        </span>
      }
      actions={actions}
    />
  )
}
