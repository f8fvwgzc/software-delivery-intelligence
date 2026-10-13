import { useSuspenseQuery } from '@tanstack/react-query'
import { useParams } from 'react-router'

import { Markdown } from '@/components/shared/data/markdown'
import { QueryBoundary } from '@/components/shared/feedback'
import { Page } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { PanelSkeleton } from '@/components/system/panel'
import { Tag } from '@/components/system/tag'
import { Skeleton } from '@/components/ui/skeleton'

import { reviewQueries } from '../api/queries'
import { ApplyFixesDialog } from '../components/apply-fixes-dialog'
import { FindingsList } from '../components/findings-list'
import { MergeFrame } from '../components/merge-frame'
import { MergeSkeleton } from '../components/merge-frame/skeleton'
import { PublishDialog } from '../components/publish-dialog'
import { ReviewHeader } from '../components/review-header'
import { ReviewRail } from '../components/review-rail'
import { isReviewRunning } from '../lib/labels'

const GRID_CLASS = 'grid items-start gap-4 lg:grid-cols-[18rem_minmax(0,1fr)] xl:grid-cols-[20rem_minmax(0,1fr)]'
/**
 * The aside stays on screen while the findings scroll. On a short window it
 * scrolls by itself, so the merge controls are always reachable; the 1px
 * padding keeps the frames' hairlines from being clipped.
 */
const ASIDE_CLASS =
  'flex min-w-0 flex-col gap-4 lg:sticky lg:top-[calc(var(--header-h)+1rem)] lg:-m-px lg:max-h-[calc(100svh-var(--header-h)-2rem)] lg:overflow-y-auto lg:p-px'

/**
 * /reviews/:reviewId — the rail and the merge frame at the left, findings and
 * summary at the right. Two requests: the review, and the pull request's
 * merge readiness behind its own boundary.
 */
export default function ReviewPage() {
  const { reviewId = '' } = useParams()

  return (
    <QueryBoundary fallback={<ReviewPageSkeleton />}>
      <ReviewView key={reviewId} reviewId={reviewId} />
    </QueryBoundary>
  )
}

function ReviewView({ reviewId }: { reviewId: string }) {
  const { data: review } = useSuspenseQuery(reviewQueries.detail(reviewId))
  const running = isReviewRunning(review.status)
  const pending = review.findings.filter((finding) => finding.decision === 'pending' || finding.decision === 'unspecified').length

  return (
    <Page>
      <ReviewHeader
        review={review}
        actions={
          <>
            <ApplyFixesDialog review={review} />
            <PublishDialog review={review} />
          </>
        }
      />

      <div className={GRID_CLASS}>
        <div className={ASIDE_CLASS}>
          <Frame title="Steps">
            <ReviewRail review={review} />
          </Frame>
          <MergeFrame review={review} />
        </div>

        <div className="flex min-w-0 flex-col gap-4">
          <Frame
            title="Findings"
            padding="none"
            description={running ? 'The analyzers are still running.' : 'Accept what should reach GitHub; dismiss the rest with a note.'}
            meta={
              review.findingsTotal > 0 ? (
                <>
                  <Tag variant="outline">{review.findingsTotal} total</Tag>
                  {pending > 0 ? <Tag tone="warning">{pending} to decide</Tag> : null}
                  <Tag tone="success">{review.findingsAccepted} accepted</Tag>
                </>
              ) : undefined
            }
          >
            <FindingsList review={review} />
          </Frame>

          <Frame title="Summary" description="The walkthrough posted as the review body.">
            {review.summary ? (
              <Markdown>{review.summary}</Markdown>
            ) : (
              <p className="py-4 text-center text-xs text-muted-foreground">
                {running ? 'Written once the analyzers finish.' : 'No summary was produced for this review.'}
              </p>
            )}
          </Frame>
        </div>
      </div>
    </Page>
  )
}

function ReviewPageSkeleton() {
  return (
    <Page>
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Skeleton className="h-3 w-8" />
          <Skeleton className="h-5 w-24 rounded-[4px]" />
        </div>
        <Skeleton className="h-5 w-80 max-w-full" />
        <Skeleton className="h-3 w-96 max-w-full" />
      </div>
      <div className={GRID_CLASS}>
        <div className="flex flex-col gap-4">
          <Frame title="Steps">
            <PanelSkeleton rows={5} />
          </Frame>
          <Frame title="Merge">
            <MergeSkeleton />
          </Frame>
        </div>
        <div className="flex flex-col gap-4">
          <Frame title="Findings" padding="none">
            <PanelSkeleton rows={4} />
          </Frame>
          <Frame title="Summary">
            <PanelSkeleton rows={2} />
          </Frame>
        </div>
      </div>
    </Page>
  )
}
