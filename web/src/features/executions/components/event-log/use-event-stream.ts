import { useEffect, useEffectEvent, useRef, useState } from 'react'

import type { components } from '@/lib/api/schema'

export type ExecutionEvent = components['schemas']['ExecutionEvent']
export type StreamState = 'idle' | 'connecting' | 'live' | 'reconnecting' | 'ended'

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

function streamUrl(executionId: string, afterSeq: number) {
  return `/api/v1/executions/${encodeURIComponent(executionId)}/events/stream?afterSeq=${afterSeq}`
}

/**
 * Subscribes to the gateway's SSE stream, resumes from the last seen `seq`
 * after a dropped connection (exponential backoff) and stops on `end`.
 */
export function useEventStream({ executionId, afterSeq, enabled, onEvent, onEnd }: UseEventStreamOptions) {
  const [events, setEvents] = useState<ExecutionEvent[]>([])
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
      source = new EventSource(streamUrl(executionId, cursorRef.current))

      source.onopen = () => {
        attempt = 0
        setState('live')
      }

      source.onmessage = (message: MessageEvent<string>) => {
        let event: ExecutionEvent
        try {
          event = JSON.parse(message.data) as ExecutionEvent
        } catch {
          return
        }
        if (event.seq <= cursorRef.current) return
        cursorRef.current = event.seq
        setEvents((previous) => [...previous, event])
        emitEvent(event)
      }

      source.addEventListener('end', () => {
        stopped = true
        close()
        setState('ended')
        emitEnd()
      })

      // Fires for both a dropped connection and the gateway's named `error` event.
      source.onerror = () => {
        if (stopped) return
        close()
        attempt += 1
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

  return { events, state: enabled ? state : 'ended' }
}

/** Initial page plus the live tail, deduplicated and in sequence order. */
export function mergeEvents(initial: ExecutionEvent[], streamed: ExecutionEvent[]) {
  if (streamed.length === 0) return initial
  const bySeq = new Map<number, ExecutionEvent>()
  for (const event of initial) bySeq.set(event.seq, event)
  for (const event of streamed) bySeq.set(event.seq, event)
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq)
}
