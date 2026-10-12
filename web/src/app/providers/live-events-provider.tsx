import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useSyncExternalStore, type ReactNode } from 'react'

import { useAuth } from '@/features/auth/lib/auth-context'
import type { components } from '@/lib/api/schema'
import { liveConnection } from '@/lib/live/connection-store'
import { createInvalidationCoalescer, type Family, type InvalidationCoalescer } from '@/lib/live/invalidation-coalescer'
import { emitPlatformEvent } from '@/lib/live/platform-events'
import { useEventSource } from '@/lib/live/use-event-source'
import { LiveEventsContext } from '@/lib/live/use-live-events'

type PlatformEvent = components['schemas']['PlatformEvent']

const STREAM_URL = '/api/v1/events/stream'
/** The stream is per workspace: a different id is a different URL, so the hook reopens it. */
const streamUrl = (workspaceId: string) => `${STREAM_URL}?workspace=${encodeURIComponent(workspaceId)}`
/** The platform stream never ends; nothing closes it but unmount. */
const NO_END_EVENTS: readonly string[] = []

/** Entity → query families to invalidate (CONTRACT.md §8, "Live events"). */
const FAMILIES_BY_ENTITY: Record<string, readonly Family[]> = {
  project: [['projects'], ['scans']],
  scan: [['projects'], ['scans']],
  proposal: [['proposals'], ['decisions'], ['documents']],
  decision: [['proposals'], ['decisions'], ['documents']],
  document: [['proposals'], ['decisions'], ['documents']],
  execution: [['executions'], ['approvals']],
  approval: [['executions'], ['approvals']],
  pattern: [['patterns']],
  diagram: [['diagrams']],
  review: [['reviews'], ['pull-requests'], ['activity']],
  finding: [['reviews'], ['activity']],
  // Continuous delivery (CONTRACT.md §22): the board, the releases and the deployment history.
  deployment: [['delivery-cd', 'pipeline'], ['delivery-cd', 'runs'], ['delivery-cd', 'releases']],
  release: [['delivery-cd', 'pipeline'], ['delivery-cd', 'releases']],
  environment: [['delivery-cd', 'pipeline']],
}

/** Every family the stream can touch; refetched when the stream comes up or drops. */
const ALL_FAMILIES: readonly Family[] = [
  ['projects'],
  ['scans'],
  ['proposals'],
  ['decisions'],
  ['documents'],
  ['executions'],
  ['approvals'],
  ['patterns'],
  ['diagrams'],
  ['reviews'],
  ['pull-requests'],
  ['activity'],
  ['delivery-cd'],
]

const isHidden = () => typeof document !== 'undefined' && document.hidden

/**
 * Opens the platform event stream once per tab and workspace and turns every
 * event into an invalidation of the affected query families, coalesced to at
 * most four flushes a second however fast events arrive. While the document
 * is hidden nothing is refetched: the families pile up and are flushed the
 * moment the tab is visible again (`invalidateQueries` only refetches the
 * queries that are mounted, so that is exactly the visible ones). While the
 * stream is up, feature queries stop polling (they read `liveConnection`);
 * when it drops they resume, and when it reconnects everything is refetched
 * once to cover whatever was missed in between.
 */
export function LiveEventsProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const { currentWorkspace } = useAuth()
  const wasConnected = useRef(false)
  const coalescerRef = useRef<InvalidationCoalescer | null>(null)

  const coalescer = () => {
    coalescerRef.current ??= createInvalidationCoalescer({
      isHidden,
      onFlush: (families) => {
        for (const family of families) queryClient.invalidateQueries({ queryKey: family })
      },
    })
    return coalescerRef.current
  }

  useEventSource<PlatformEvent>({
    url: streamUrl(currentWorkspace.id),
    endEvents: NO_END_EVENTS,
    onMessage: (event) => {
      liveConnection.markEvent(Date.now())
      emitPlatformEvent(event)
      const families = FAMILIES_BY_ENTITY[event.entity]
      if (!families) return
      for (const family of families) coalescer().add(family)
    },
    onStateChange: (state) => {
      const connected = state === 'open'
      liveConnection.setConnected(connected)
      liveConnection.setPaused(state === 'paused')
      // A refetch on each edge re-evaluates every `refetchInterval` fallback,
      // so polling stops the moment the stream is up and resumes when it drops.
      // A pause (hidden tab) is not an edge: nothing should fetch while hidden.
      if (connected || (wasConnected.current && state !== 'paused')) {
        for (const family of ALL_FAMILIES) coalescer().add(family)
      }
      wasConnected.current = connected
    },
  })

  // Hidden → visible releases whatever the stream reported in the meantime.
  useEffect(() => {
    const onVisibilityChange = () => {
      if (!document.hidden) coalescerRef.current?.resume()
    }
    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChange)
      coalescerRef.current?.dispose()
      coalescerRef.current = null
    }
  }, [])

  const value = useSyncExternalStore(liveConnection.subscribe, liveConnection.get)

  return <LiveEventsContext.Provider value={value}>{children}</LiveEventsContext.Provider>
}
