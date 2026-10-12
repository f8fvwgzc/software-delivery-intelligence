import { Rail, RailStep, type RailStatus } from '@/components/system/rail'
import { Tag } from '@/components/system/tag'

import type { Review, ReviewStep } from '../../api/types'
import { REVIEW_STEPS, formatDurationMs, stepRailStatus } from '../../lib/labels'

/**
 * The five steps of a review on the shared rail. A review reports the steps
 * it has reached; the rest are drawn pending so the shape never changes.
 */
export function ReviewRail({ review }: { review: Review }) {
  const reported = new Map<string, ReviewStep>(review.steps.map((step) => [step.key, step]))

  return (
    <Rail aria-label="Review steps">
      {REVIEW_STEPS.map((definition) => {
        const step: ReviewStep | undefined = reported.get(definition.key)
        const status: RailStatus = step ? stepRailStatus(step.status) : publishPending(definition.key, review)
        const detail = step?.detail || definition.hint
        const duration = step ? formatDurationMs(step.durationMs) : ''

        return (
          <RailStep
            key={definition.key}
            status={status}
            title={step?.label || definition.label}
            compact
            dimmed={status === 'pending'}
            trailing={duration ? <span className="font-mono text-2xs tabular-nums text-muted-foreground">{duration}</span> : undefined}
            meta={
              definition.key === 'findings' && review.findingsTotal > 0 ? (
                <Tag variant="outline">
                  {review.findingsAccepted}/{review.findingsTotal} accepted
                </Tag>
              ) : undefined
            }
            detail={status === 'failed' && review.error ? <span className="text-destructive">{review.error}</span> : detail}
          />
        )
      })}
    </Rail>
  )
}

/** The publish step waits on the human; a published review marks it done without the service reporting it. */
function publishPending(key: string, review: Review): RailStatus {
  if (key === 'publish' && review.status === 'published') return 'done'
  if (key === 'publish' && review.status === 'proposed') return 'waiting'
  if (key === 'findings' && review.status === 'proposed') return 'waiting'
  return 'pending'
}
