import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useSyncExternalStore, type ReactNode } from 'react'

import { useAuth } from '@/features/auth/lib/auth-context'
import type { components } from '@/lib/api/schema'
import { liveConnection } from '@/lib/live/connection-store'
import { useEventSource } from '@/lib/live/use-event-source'
import { LiveEventsContext } from '@/lib/live/use-live-events'

type PlatformEvent = components['schemas']['PlatformEvent']

const STREAM_URL = '/api/v1/events/stream'
/** The stream is per workspace: a different id is a different URL, so the hook reopens it. */
const streamUrl = (workspaceId: string) => `${STREAM_URL}?workspace=${encodeURIComponent(workspaceId)}`
const DEBOUNCE_MS = 150
/** The platform stream never ends; nothing closes it but unmount. */
const NO_END_EVENTS: readonly string[] = []

/** Entity → query families to invalidate (CONTRACT.md §8, "Live events"). */
const FAMILIES_BY_ENTITY: Record<string, readonly (readonly string[])[]> = {
  project: [['projects'], ['scans']],
  scan: [['projects'], ['scans']],
  proposal: [['proposals'], ['decisions'], ['documents']],
  decision: [['proposals'], ['decisions'], ['documents']],
  document: [['proposals'], ['decisions'], ['documents']],
  execution: [['executions'], ['approvals']],
  approval: [['executions'], ['approvals']],
  pattern: [['patterns']],
  diagram: [['diagrams']],
}

/** Every family the stream can touch; refetched when the stream comes up or drops. */
const ALL_FAMILIES: readonly (readonly string[])[] = [
  ['projects'],
  ['scans'],
  ['proposals'],
  ['decisions'],
  ['documents'],
  ['executions'],
  ['approvals'],
  ['patterns'],
  ['diagrams'],
]

/**
 * Opens the platform event stream once per tab and workspace and turns every event into
 * a debounced invalidation of the affected query families. While the stream
 * is up, feature queries stop polling (they read `liveConnection`); when it
 * drops they resume, and when it reconnects everything is refetched once to
 * cover whatever was missed in between.
 */
export function LiveEventsProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const { currentWorkspace } = useAuth()
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>())
  const wasConnected = useRef(false)

  const invalidateFamily = (family: readonly string[]) => {
    const key = family.join('/')
    const pending = timers.current.get(key)
    if (pending) clearTimeout(pending)
    timers.current.set(
      key,
      setTimeout(() => {
        timers.current.delete(key)
        queryClient.invalidateQueries({ queryKey: family })
      }, DEBOUNCE_MS),
    )
  }

  useEventSource<PlatformEvent>({
    url: streamUrl(currentWorkspace.id),
    endEvents: NO_END_EVENTS,
    onMessage: (event) => {
      liveConnection.markEvent(Date.now())
      const families = FAMILIES_BY_ENTITY[event.entity]
      if (!families) return
      for (const family of families) invalidateFamily(family)
    },
    onStateChange: (state) => {
      const connected = state === 'open'
      liveConnection.setConnected(connected)
      liveConnection.setPaused(state === 'paused')
      // A refetch on each edge re-evaluates every `refetchInterval` fallback,
      // so polling stops the moment the stream is up and resumes when it drops.
      // A pause (hidden tab) is not an edge: nothing should fetch while hidden.
      if (connected || (wasConnected.current && state !== 'paused')) {
        for (const family of ALL_FAMILIES) invalidateFamily(family)
      }
      wasConnected.current = connected
    },
  })

  // Pending debounces die with the provider.
  useEffect(() => {
    const pending = timers.current
    return () => {
      for (const timer of pending.values()) clearTimeout(timer)
      pending.clear()
    }
  }, [])

  const value = useSyncExternalStore(liveConnection.subscribe, liveConnection.get)

  return <LiveEventsContext.Provider value={value}>{children}</LiveEventsContext.Provider>
}
