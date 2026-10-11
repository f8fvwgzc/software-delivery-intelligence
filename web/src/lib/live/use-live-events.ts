import { createContext, useContext } from 'react'

import type { LiveConnection } from './connection-store'

/**
 * Whether the platform event stream (`GET /events/stream`) is connected,
 * and when it last delivered an event. Provided once by
 * `LiveEventsProvider`; read by the sidebar dot and by anything that wants
 * to show "live" versus "polling".
 */
export const LiveEventsContext = createContext<LiveConnection>({ connected: false, paused: false, lastEventAt: null })

export function useLiveEvents(): LiveConnection {
  return useContext(LiveEventsContext)
}
