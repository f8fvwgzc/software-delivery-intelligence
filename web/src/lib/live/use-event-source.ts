import { useEffect, useEffectEvent, useState } from 'react'

import { openEventSource, type EventSourceOptions, type StreamState } from './event-source'

interface UseEventSourceOptions<T> extends EventSourceOptions {
  /** False keeps the stream closed (e.g. the entity is already terminal). */
  enabled?: boolean
  /** Turns a `data:` line into a value; defaults to `JSON.parse`. Throwing drops the line. */
  parse?: (data: string) => T
  onMessage: (value: T) => void
  onEnd?: (name: string) => void
  /** Every connection transition, before the hook's own state updates. */
  onStateChange?: (state: StreamState) => void
}

export interface EventSourceStatus {
  state: StreamState
  /** True only while the connection is open. */
  connected: boolean
  /** Epoch milliseconds of the last message, or null before the first. */
  lastEventAt: number | null
}

const parseJson = <T>(data: string) => JSON.parse(data) as T

/**
 * Subscribes to one SSE endpoint for the lifetime of the component (or
 * while `enabled`). Callbacks are effect events, so a parent re-render
 * never reopens the connection: only `url` and `enabled` do.
 */
export function useEventSource<T>({
  url,
  enabled = true,
  parse = parseJson,
  onMessage,
  onEnd,
  onStateChange,
  endEvents,
  minBackoffMs,
  maxBackoffMs,
  hiddenGraceMs,
  maxRefusals,
  createSource,
}: UseEventSourceOptions<T>): EventSourceStatus {
  const [state, setState] = useState<StreamState>(enabled ? 'connecting' : 'closed')
  const [lastEventAt, setLastEventAt] = useState<number | null>(null)

  const handleMessage = useEffectEvent((data: string) => {
    let value: T
    try {
      value = parse(data)
    } catch {
      return
    }
    setLastEventAt(Date.now())
    onMessage(value)
  })
  const handleEnd = useEffectEvent((name: string) => onEnd?.(name))
  const handleState = useEffectEvent((next: StreamState) => {
    onStateChange?.(next)
    setState(next)
  })

  useEffect(() => {
    if (!enabled) return
    return openEventSource(
      { url, endEvents, minBackoffMs, maxBackoffMs, hiddenGraceMs, maxRefusals, createSource },
      { onMessage: handleMessage, onEnd: handleEnd, onStateChange: handleState },
    )
  }, [url, enabled, endEvents, minBackoffMs, maxBackoffMs, hiddenGraceMs, maxRefusals, createSource])

  const effectiveState = enabled ? state : 'closed'
  return { state: effectiveState, connected: effectiveState === 'open', lastEventAt }
}
