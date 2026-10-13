import { TagDot } from '@/components/system/tag'

import type { MergeReadiness } from '../../api/types'
import { blockerTone } from '../../lib/merge'

/** Codes whose blocker passes by itself: something is running. */
const IN_FLIGHT: ReadonlySet<string> = new Set(['review_running', 'merge_in_progress', 'checks_pending', 'mergeability_unknown'])

/** What stands in the way, GitHub's reasons first, each as the sentence the server wrote. */
export function MergeBlockers({ readiness }: { readiness: MergeReadiness }) {
  if (readiness.canMerge) {
    return (
      <p className="flex items-start gap-2 text-xs leading-4">
        <TagDot tone="success" className="mt-[5px]" />
        Nothing stands in the way.
      </p>
    )
  }

  if (readiness.blockers.length === 0) {
    return <p className="text-xs leading-4 text-muted-foreground">This pull request cannot be merged yet.</p>
  }

  return (
    <ul aria-label="What stands in the way" className="flex flex-col gap-1.5">
      {readiness.blockers.map((blocker) => (
        <li key={blocker.code} className="flex items-start gap-2 text-xs leading-4">
          <TagDot tone={blockerTone(blocker.code)} live={IN_FLIGHT.has(blocker.code)} className="mt-[5px]" />
          <span className="min-w-0">{blocker.message}</span>
        </li>
      ))}
    </ul>
  )
}
