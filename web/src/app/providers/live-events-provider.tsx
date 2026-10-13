import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useSyncExternalStore, type ReactNode } from 'react'

import { useAuth } from '@/features/auth/lib/auth-context'
import type { components } from '@/lib/api/schema'
import { liveConnection } from '@/lib/live/connection-store'
import { ALL_FAMILIES, ALL_GATED_FAMILIES, FAMILIES_BY_ENTITY, gatedFamilies } from '@/lib/live/event-families'
import { createInvalidationCoalescer, type CoalescerOptions, type InvalidationCoalescer } from '@/lib/live/invalidation-coalescer'
import { emitPlatformEvent } from '@/lib/live/platform-events'
import { useEventSource } from '@/lib/live/use-event-source'
import { LiveEventsContext } from '@/lib/live/use-live-events'

type PlatformEvent = components['schemas']['PlatformEvent']

const STREAM_URL = '/api/v1/events/stream'
/** The stream is per workspace: a different id is a different URL, so the hook reopens it. */
const streamUrl = (workspaceId: string) => `${STREAM_URL}?workspace=${encodeURIComponent(workspaceId)}`
/** The platform stream never ends; nothing closes it but unmount. */
const NO_END_EVENTS: readonly string[] = []

/**
 * Spacing of the gated lane (lib/live/event-families.ts): merge readiness and
 * the security overview are refetched once, a second after the first event
 * of a burst, however many events the burst holds.
 */
const GATED_INTERVAL_MS = 1_000

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
 *
 * The answers the server has to fetch from GitHub (merge readiness, the
 * security overview) go through a second, slower lane that only the events
 * able to change them enter: see `gatedFamilies`.
 */
export function LiveEventsProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const { currentWorkspace } = useAuth()
  const wasConnected = useRef(false)
  const hasConnected = useRef(false)
  const coalescerRef = useRef<InvalidationCoalescer | null>(null)
  const gatedRef = useRef<InvalidationCoalescer | null>(null)

  const onFlush: CoalescerOptions['onFlush'] = (families) => {
    for (const family of families) queryClient.invalidateQueries({ queryKey: family })
  }
  const coalescer = () => {
    coalescerRef.current ??= createInvalidationCoalescer({ isHidden, onFlush })
    return coalescerRef.current
  }
  const gated = () => {
    gatedRef.current ??= createInvalidationCoalescer({ isHidden, onFlush, intervalMs: GATED_INTERVAL_MS, leading: false })
    return gatedRef.current
  }

  useEventSource<PlatformEvent>({
    url: streamUrl(currentWorkspace.id),
    endEvents: NO_END_EVENTS,
    onMessage: (event) => {
      liveConnection.markEvent(Date.now())
      emitPlatformEvent(event)
      for (const family of FAMILIES_BY_ENTITY[event.entity] ?? []) coalescer().add(family)
      for (const family of gatedFamilies(event)) gated().add(family)
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
      // The gated lane has no polling to re-evaluate: it only needs the one
      // refetch after a reconnect, for an event missed while the stream was
      // away. The first connection of a stream has missed nothing.
      if (connected && hasConnected.current) {
        for (const family of ALL_GATED_FAMILIES) gated().add(family)
      }
      if (connected) hasConnected.current = true
      // Closed for good (unmount, another workspace): the next stream starts over.
      if (state === 'closed') hasConnected.current = false
      wasConnected.current = connected
    },
  })

  // Hidden → visible releases whatever the stream reported in the meantime.
  useEffect(() => {
    const onVisibilityChange = () => {
      if (document.hidden) return
      coalescerRef.current?.resume()
      gatedRef.current?.resume()
    }
    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChange)
      coalescerRef.current?.dispose()
      coalescerRef.current = null
      gatedRef.current?.dispose()
      gatedRef.current = null
    }
  }, [])

  const value = useSyncExternalStore(liveConnection.subscribe, liveConnection.get)

  return <LiveEventsContext.Provider value={value}>{children}</LiveEventsContext.Provider>
}
