/**
 * The platform stream's connection state as a tiny external store.
 *
 * It exists outside React so query definitions (`refetchInterval` in a
 * feature's `api/queries.ts`) can ask "is the live stream up?" without a
 * hook, and so the provider and the context share one source of truth.
 */

export interface LiveConnection {
  connected: boolean
  /** True while the stream is deliberately closed because the tab is hidden. */
  paused: boolean
  /** Epoch milliseconds of the last platform event, or null before the first. */
  lastEventAt: number | null
}

type Listener = () => void

let snapshot: LiveConnection = { connected: false, paused: false, lastEventAt: null }
const listeners = new Set<Listener>()

function emit() {
  for (const listener of listeners) listener()
}

export const liveConnection = {
  get: (): LiveConnection => snapshot,

  subscribe: (listener: Listener) => {
    listeners.add(listener)
    return () => {
      listeners.delete(listener)
    }
  },

  setConnected: (connected: boolean) => {
    if (snapshot.connected === connected) return
    snapshot = { ...snapshot, connected }
    emit()
  },

  setPaused: (paused: boolean) => {
    if (snapshot.paused === paused) return
    snapshot = { ...snapshot, paused }
    emit()
  },

  markEvent: (at: number) => {
    snapshot = { ...snapshot, lastEventAt: at }
    emit()
  },

  /** For `refetchInterval` fallbacks: poll only while the stream is down. */
  isConnected: () => snapshot.connected,
}

/**
 * `refetchInterval` helper: the interval while the live stream is
 * disconnected, otherwise off. A paused stream (hidden tab) is not down:
 * nothing should fetch while nobody is looking, and the stream reconnects
 * the moment the tab is visible again.
 */
export function fallbackPoll(intervalMs: number): number | false {
  const { connected, paused } = liveConnection.get()
  return connected || paused ? false : intervalMs
}
