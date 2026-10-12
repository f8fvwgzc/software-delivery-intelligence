import { useSuspenseQuery } from '@tanstack/react-query'
import { useParams } from 'react-router'

import { reviewQueries } from '../api/queries'

/** Breadcrumb for `/reviews/:reviewId`: "#42 Add rate limiting". */
export default function ReviewCrumb() {
  const { reviewId = '' } = useParams()
  const { data: review } = useSuspenseQuery(reviewQueries.detail(reviewId))

  return (
    <span className="inline-flex max-w-64 items-baseline gap-1.5">
      <span className="font-mono tabular-nums text-muted-foreground">#{review.prNumber}</span>
      <span className="truncate">{review.prTitle}</span>
    </span>
  )
}
