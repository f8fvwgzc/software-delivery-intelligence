import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { GitPullRequestIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime } from '@/components/shared/data/relative-time'
import { PanelEmpty } from '@/components/system/panel'
import { Row, RowList } from '@/components/system/row'
import { Tag, TagDot } from '@/components/system/tag'
import { Button } from '@/components/ui/button'

import { reviewQueries } from '../../api/queries'
import { REVIEW_STATUS, shortSha } from '../../lib/labels'
import { count } from '@/lib/intl'

interface ReviewsListProps {
  projectId?: string
}

/** Every review, newest first: PR, state, findings, when. */
export function ReviewsList({ projectId }: ReviewsListProps) {
  const { data: reviews } = useSuspenseQuery(reviewQueries.list(projectId ? { projectId } : {}))
  const queryClient = useQueryClient()

  if (reviews.length === 0) {
    return (
      <PanelEmpty
        icon={GitPullRequestIcon}
        title="No reviews yet"
        description="Start one from a project's pull requests. Findings wait for you before anything reaches GitHub."
        action={
          projectId ? (
            <Button asChild variant="outline" size="sm">
              <Link to={routes.projectPullRequests(projectId)}>Open pull requests</Link>
            </Button>
          ) : (
            <Button asChild variant="outline" size="sm">
              <Link to={routes.projects()}>Open projects</Link>
            </Button>
          )
        }
      />
    )
  }

  return (
    <RowList stagger>
      {reviews.map((review) => {
        const status = REVIEW_STATUS[review.status] ?? REVIEW_STATUS.unspecified
        return (
          <li key={review.id}>
            <Row
              to={routes.review(review.id)}
              onIntent={() => queryClient.prefetchQuery(reviewQueries.detail(review.id))}
              leading={<TagDot tone={status.tone} live={status.live} />}
              title={
                <>
                  <span className="font-mono text-xs text-muted-foreground">#{review.prNumber}</span> {review.prTitle}
                </>
              }
              meta={
                <>
                  <span>{review.projectName}</span>
                  <span>{review.prAuthor}</span>
                  <span className="font-mono">{shortSha(review.headSha)}</span>
                  <span className="tabular-nums">
                    {count(review.findingsTotal, 'finding', 'findings')}
                    {review.findingsTotal > 0 ? `, ${review.findingsAccepted} accepted` : ''}
                  </span>
                </>
              }
              trailing={
                <>
                  <Tag tone={status.tone}>{status.label}</Tag>
                  <RelativeTime value={review.updatedAt} className="font-mono tabular-nums" />
                </>
              }
            />
          </li>
        )
      })}
    </RowList>
  )
}
