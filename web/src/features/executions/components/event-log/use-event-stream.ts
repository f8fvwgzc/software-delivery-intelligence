import { useEffect, useEffectEvent, useRef, useState } from 'react'

import type { components } from '@/lib/api/schema'

export type ExecutionEvent = components['schemas']['ExecutionEvent']
export type StreamState = 'idle' | 'connecting' | 'live' | 'reconnecting' | 'unavailable' | 'ended'

interface UseEventStreamOptions {
  executionId: string
  /** Sequence number of the newest event already loaded; the stream resumes after it. */
  afterSeq: number
  /** False once the execution is terminal: nothing more will arrive. */
  enabled: boolean
  /** Called for every streamed event. */
  onEvent?: (event: ExecutionEvent) => void
  onEnd?: () => void
}

const MIN_BACKOFF = 1_000
const MAX_BACKOFF = 15_000
/** Refused connections (readyState CLOSED, which browsers do not retry) before giving up. */
const MAX_REFUSALS = 5

function streamUrl(executionId: string, afterSeq: number) {
  return `/api/v1/executions/${encodeURIComponent(executionId)}/events/stream?afterSeq=${afterSeq}`
}

/**
 * Subscribes to the gateway's SSE stream, resumes from the last seen `seq`
 * after a dropped connection (exponential backoff) and stops on `end`. The
 * events themselves go to `onEvent` only: the caller owns their store (the
 * query cache), so there is one list, not two to merge on every line.
 */
export function useEventStream({ executionId, afterSeq, enabled, onEvent, onEnd }: UseEventStreamOptions) {
  const [state, setState] = useState<StreamState>(enabled ? 'connecting' : 'ended')

  // The cursor is only touched inside the effect; callbacks are effect events
  // so the subscription does not re-run when the parent re-renders.
  const cursorRef = useRef(0)
  const emitEvent = useEffectEvent((event: ExecutionEvent) => onEvent?.(event))
  const emitEnd = useEffectEvent(() => onEnd?.())

  useEffect(() => {
    if (!enabled) return
    if (afterSeq > cursorRef.current) cursorRef.current = afterSeq

    let source: EventSource | null = null
    let timer: ReturnType<typeof setTimeout> | undefined
    let attempt = 0
    let stopped = false

    const close = () => {
      source?.close()
      source = null
    }

    const connect = () => {
      setState(attempt === 0 ? 'connecting' : 'reconnecting')
      const next = new EventSource(streamUrl(executionId, cursorRef.current))
      source = next

      next.onopen = () => {
        attempt = 0
        setState('live')
      }

      next.onmessage = (message: MessageEvent<string>) => {
        let event: ExecutionEvent
        try {
          event = JSON.parse(message.data) as ExecutionEvent
        } catch {
          return
        }
        if (event.seq <= cursorRef.current) return
        cursorRef.current = event.seq
        emitEvent(event)
      }

      next.addEventListener('end', () => {
        stopped = true
        close()
        setState('ended')
        emitEnd()
      })

      // Fires for both a dropped connection and the gateway's named `error` event.
      next.onerror = () => {
        if (stopped || next !== source) return
        const refused = next.readyState === EventSource.CLOSED
        close()
        attempt += 1
        if (refused && attempt >= MAX_REFUSALS) {
          // The gateway will not serve this stream; the detail query's
          // fallback poll keeps the page current instead.
          stopped = true
          setState('unavailable')
          return
        }
        setState('reconnecting')
        const delay = Math.min(MIN_BACKOFF * 2 ** (attempt - 1), MAX_BACKOFF)
        timer = setTimeout(connect, delay)
      }
    }

    connect()

    return () => {
      stopped = true
      close()
      if (timer) clearTimeout(timer)
    }
  }, [executionId, enabled, afterSeq])

  return { state: enabled ? state : 'ended' }
}

/**
 * Appends a streamed event to the cached page. Both sources are in `seq`
 * order, so the common case is a push; an out-of-order arrival (a resumed
 * stream overlapping the page) is placed, and a duplicate is dropped.
 */
export function appendEvent(cached: ExecutionEvent[], event: ExecutionEvent): ExecutionEvent[] {
  const last = cached[cached.length - 1]
  if (!last || last.seq < event.seq) return [...cached, event]
  if (cached.some((known) => known.seq === event.seq)) return cached
  const index = cached.findIndex((known) => known.seq > event.seq)
  return [...cached.slice(0, index), event, ...cached.slice(index)]
}

/**
 * Appends a batch in one pass: one copy of the cached list however many
 * events arrived in the frame. Events already in order are pushed; the
 * rare out-of-order one goes through `appendEvent`.
 */
export function appendEvents(cached: ExecutionEvent[], batch: ExecutionEvent[]): ExecutionEvent[] {
  if (batch.length === 0) return cached
  let next = cached.slice()
  for (const event of batch) {
    const last = next[next.length - 1]
    if (!last || last.seq < event.seq) next.push(event)
    else next = appendEvent(next, event)
  }
  return next
}
