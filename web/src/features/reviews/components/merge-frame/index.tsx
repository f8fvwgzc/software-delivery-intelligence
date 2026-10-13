import { Panel } from '@/components/system/panel'

import type { Review } from '../../api/types'
import { MergeBody } from './merge-body'
import { MergeSkeleton } from './skeleton'

/**
 * The last step of a pull request on its review page: whether it can be
 * merged now, what stands in the way, and the request to merge it
 * (CONTRACT.md §23.3). Its own boundary: the answer is read from GitHub at
 * that moment, and the review around it renders without waiting for it.
 */
export function MergeFrame({ review }: { review: Review }) {
  return (
    <Panel title="Merge" skeleton={<MergeSkeleton />}>
      <MergeBody review={review} />
    </Panel>
  )
}
