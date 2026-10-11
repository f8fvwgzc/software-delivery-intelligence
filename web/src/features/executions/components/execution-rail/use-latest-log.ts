import { useQuery } from '@tanstack/react-query'

import type { components } from '@/lib/api/schema'

import { executionQueries } from '../../api/queries'

type ExecutionEvent = components['schemas']['ExecutionEvent']

/** Step key → message of its newest `log` event. Stable reference, so `select` memoizes. */
function latestLogByStep(events: ExecutionEvent[]) {
  const latest: Record<string, string> = {}
  for (const event of events) {
    if (event.type === 'log' && event.stepKey) latest[event.stepKey] = event.message
  }
  return latest
}

/**
 * The newest log line per step, read from the events cache the event log
 * keeps fed over SSE. Never suspends: the rail paints before the log loads.
 */
export function useLatestLogByStep(executionId: string) {
  const { data } = useQuery({ ...executionQueries.events(executionId), select: latestLogByStep })
  return data ?? {}
}
