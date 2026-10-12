/**
 * Every platform event the stream delivers, fanned out to whoever asked.
 * The live-events provider feeds it; features that need the event itself
 * (not only the invalidation it causes), such as the proposal page's
 * generation progress, subscribe through `usePlatformEvents`.
 */
import { useEffect } from 'react'

import type { components } from '@/lib/api/schema'

export type PlatformEvent = components['schemas']['PlatformEvent']

type Listener = (event: PlatformEvent) => void

const listeners = new Set<Listener>()

export function emitPlatformEvent(event: PlatformEvent) {
  for (const listener of listeners) listener(event)
}

export function onPlatformEvent(listener: Listener) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** Calls `listener` for every event while mounted; the listener is read fresh on each event. */
export function usePlatformEvents(listener: Listener) {
  useEffect(() => onPlatformEvent(listener), [listener])
}
