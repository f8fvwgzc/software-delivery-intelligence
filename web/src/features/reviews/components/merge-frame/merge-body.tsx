import { useSuspenseQuery } from '@tanstack/react-query'
import { RefreshCwIcon } from 'lucide-react'
import { useRef, useState } from 'react'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { Button } from '@/components/ui/button'
import { useInViewport } from '@/hooks/use-in-viewport'

import { mergeQueries } from '../../api/queries'
import type { Review } from '../../api/types'
import { isClosed, needsRecheck, RECHECK_INTERVAL_MS } from '../../lib/merge'
import { ExecutionLink } from '@/components/shared/data/execution-link'
import { MergeBlockers } from './merge-blockers'
import { MergeControls } from './merge-controls'
import { MergeState } from './merge-state'

/**
 * GitHub's state, the blockers, and either the controls or the way to the
 * merge that is already running. While GitHub is still computing
 * mergeability or a check is still running, the answer is asked for again
 * every 10 s, but only while the frame is on screen; in every other state
 * nothing polls, and "Check again" asks once.
 */
export function MergeBody({ review }: { review: Review }) {
  const ref = useRef<HTMLDivElement>(null)
  const onScreen = useInViewport(ref)
  const [checking, setChecking] = useState(false)
  const {
    data: readiness,
    refetch,
    isRefetchError,
  } = useSuspenseQuery({
    ...mergeQueries.readiness(review.projectId, review.prNumber),
    refetchInterval: (query) => (onScreen && needsRecheck(query.state.data) ? RECHECK_INTERVAL_MS : false),
  })
  const waiting = needsRecheck(readiness)

  const check = async () => {
    setChecking(true)
    try {
      await refetch()
    } finally {
      setChecking(false)
    }
  }

  return (
    <div ref={ref} className="flex flex-col gap-3">
      <MergeState readiness={readiness} />
      <MergeBlockers readiness={readiness} />

      {readiness.mergeExecutionId ? (
        <div className="flex flex-col items-start gap-1">
          <p className="text-xs leading-4 text-muted-foreground">The merge runs as an execution. Its approval and its outcome are there.</p>
          <ExecutionLink executionId={readiness.mergeExecutionId}>Open the merge run</ExecutionLink>
        </div>
      ) : isClosed(readiness) ? null : (
        <MergeControls review={review} readiness={readiness} />
      )}

      {isRefetchError ? (
        <p role="status" className="text-2xs leading-4 text-warning">
          GitHub could not be read just now. This is the last answer.
        </p>
      ) : null}

      {waiting ? <p className="text-2xs leading-4 text-muted-foreground">Asks again every 10 seconds while this is on screen.</p> : null}

      <footer className="-mx-3 -mb-3 flex min-h-8 items-center justify-between gap-2 border-t py-1 pr-1.5 pl-3 text-2xs text-muted-foreground">
        <span className="min-w-0 truncate">
          Read from GitHub <RelativeTime value={readiness.checkedAt} className="tabular-nums" />
        </span>
        <Button variant="ghost" size="xs" onClick={check} loading={checking} className="shrink-0">
          <RefreshCwIcon data-icon="inline-start" />
          Check again
        </Button>
      </footer>
    </div>
  )
}
