/**
 * A resilient wrapper around the browser's EventSource.
 *
 * - Reconnects with exponential backoff (1s → 30s) plus ±20% jitter so a
 *   fleet of tabs does not stampede the gateway after a restart.
 * - Pauses once the document has been hidden for longer than the grace
 *   period (60s) and reconnects as soon as it is visible again, so a
 *   background tab does not hold a long-lived connection for hours.
 * - Reconnects at once when the browser reports it is back online.
 * - Gives up after a few attempts when the gateway refuses the stream
 *   outright (a non-2xx or a wrong content type leaves the EventSource
 *   CLOSED, which browsers themselves do not retry), so a stale id does not
 *   reconnect forever.
 * - Stops for good on a named terminal event (`end` by default) or when the
 *   returned `close()` is called from a React cleanup.
 */

export type StreamState = 'connecting' | 'open' | 'reconnecting' | 'paused' | 'closed'

export interface EventSourceHandlers {
  /** Every unnamed `data:` line, raw. */
  onMessage: (data: string) => void
  /** A named event that ends the stream (`end`, or any name in `endEvents`). */
  onEnd?: (name: string, data: string) => void
  onStateChange?: (state: StreamState) => void
}

export interface EventSourceOptions {
  url: string
  /** Named events that close the stream for good. Default `['end']`. */
  endEvents?: readonly string[]
  minBackoffMs?: number
  maxBackoffMs?: number
  /** How long the document may stay hidden before the stream is paused. */
  hiddenGraceMs?: number
  /** Consecutive refused connections (readyState CLOSED) before giving up. */
  maxRefusals?: number
}

const DEFAULTS = {
  endEvents: ['end'] as readonly string[],
  minBackoffMs: 1_000,
  maxBackoffMs: 30_000,
  hiddenGraceMs: 60_000,
  maxRefusals: 5,
}

/** Exponential backoff with jitter, capped. */
export function backoffDelay(attempt: number, min: number, max: number) {
  const base = Math.min(min * 2 ** Math.max(0, attempt - 1), max)
  const jitter = 0.8 + Math.random() * 0.4
  return Math.round(base * jitter)
}

/**
 * Opens the stream and returns a function that closes it. Safe to call in a
 * React effect: the returned function is the cleanup.
 */
export function openEventSource(options: EventSourceOptions, handlers: EventSourceHandlers): () => void {
  const { url } = options
  const endEvents = options.endEvents ?? DEFAULTS.endEvents
  const minBackoffMs = options.minBackoffMs ?? DEFAULTS.minBackoffMs
  const maxBackoffMs = options.maxBackoffMs ?? DEFAULTS.maxBackoffMs
  const hiddenGraceMs = options.hiddenGraceMs ?? DEFAULTS.hiddenGraceMs
  const maxRefusals = options.maxRefusals ?? DEFAULTS.maxRefusals

  let source: EventSource | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined
  let hiddenTimer: ReturnType<typeof setTimeout> | undefined
  let attempt = 0
  let stopped = false
  let paused = false

  const setState = (state: StreamState) => handlers.onStateChange?.(state)

  const closeSource = () => {
    source?.close()
    source = null
  }

  const clearReconnect = () => {
    if (reconnectTimer) clearTimeout(reconnectTimer)
    reconnectTimer = undefined
  }

  const scheduleReconnect = () => {
    attempt += 1
    setState('reconnecting')
    clearReconnect()
    reconnectTimer = setTimeout(connect, backoffDelay(attempt, minBackoffMs, maxBackoffMs))
  }

  function connect() {
    if (stopped || paused) return
    clearReconnect()
    setState(attempt === 0 ? 'connecting' : 'reconnecting')

    const next = new EventSource(url)
    source = next

    next.onopen = () => {
      attempt = 0
      setState('open')
    }

    next.onmessage = (message: MessageEvent<string>) => {
      handlers.onMessage(message.data)
    }

    for (const name of endEvents) {
      next.addEventListener(name, (event) => {
        stopped = true
        closeSource()
        setState('closed')
        handlers.onEnd?.(name, (event as MessageEvent<string>).data ?? '')
      })
    }

    // Fires for a dropped connection and for the gateway's named `error` event.
    next.onerror = () => {
      if (stopped || next !== source) return
      const refused = next.readyState === EventSource.CLOSED
      closeSource()
      if (refused && attempt + 1 >= maxRefusals) {
        stopped = true
        setState('closed')
        return
      }
      scheduleReconnect()
    }
  }

  const onVisibilityChange = () => {
    if (document.hidden) {
      if (hiddenTimer) clearTimeout(hiddenTimer)
      hiddenTimer = setTimeout(() => {
        if (stopped) return
        paused = true
        clearReconnect()
        closeSource()
        setState('paused')
      }, hiddenGraceMs)
      return
    }

    if (hiddenTimer) clearTimeout(hiddenTimer)
    hiddenTimer = undefined
    if (paused) {
      paused = false
      attempt = 0
      connect()
    }
  }

  const onOnline = () => {
    // A pending backoff can be skipped: the network just came back.
    if (!stopped && !paused && reconnectTimer) {
      clearReconnect()
      connect()
    }
  }

  document.addEventListener('visibilitychange', onVisibilityChange)
  window.addEventListener('online', onOnline)
  connect()

  return () => {
    stopped = true
    clearReconnect()
    if (hiddenTimer) clearTimeout(hiddenTimer)
    document.removeEventListener('visibilitychange', onVisibilityChange)
    window.removeEventListener('online', onOnline)
    closeSource()
    setState('closed')
  }
}
