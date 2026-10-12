/*
 * Realtime scenarios: ten SSE connections that are refused or flap, driven
 * through the real `openEventSource` wrapper with a fake transport, and the
 * invalidation coalescer fed 100 events a second while the frame rate is
 * watched. Nothing touches the network.
 */
import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'

import { createInvalidationCoalescer } from '@/lib/live/invalidation-coalescer'
import { openEventSource, type EventSourceLike, type StreamState } from '@/lib/live/event-source'

import { Metrics, Scenario, type Done } from '../scenario'


export const STREAM_COUNT = 10
/** How the fake transport behaves per stream: refused (never opens) or flapping (opens, then drops). */
type Behaviour = 'refused' | 'flapping'

const CONNECTING = 0
const OPEN = 1
const CLOSED = 2

/**
 * A stand-in for `EventSource`: refused streams report `readyState`
 * CLOSED and an error shortly after construction (what a browser does on
 * a 4xx/5xx or a wrong content type); flapping streams open, drop after a
 * moment with CONNECTING (a lost connection the wrapper must reconnect).
 */
class FakeEventSource implements EventSourceLike {
  readyState = CONNECTING
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent<string>) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  private timers: ReturnType<typeof setTimeout>[] = []
  private listeners = new Map<string, Set<(event: Event) => void>>()

  constructor(behaviour: Behaviour, onAttempt: () => void) {
    onAttempt()
    if (behaviour === 'refused') {
      this.timers.push(
        setTimeout(() => {
          this.readyState = CLOSED
          this.onerror?.(new Event('error'))
        }, 10),
      )
      return
    }
    this.timers.push(
      setTimeout(() => {
        this.readyState = OPEN
        this.onopen?.(new Event('open'))
        this.onmessage?.(new MessageEvent('message', { data: '{"entity":"project","id":"x"}' }))
      }, 15),
      setTimeout(() => {
        if (this.readyState === CLOSED) return
        this.readyState = CONNECTING
        this.onerror?.(new Event('error'))
      }, 120),
    )
  }

  close() {
    this.readyState = CLOSED
    for (const timer of this.timers) clearTimeout(timer)
  }

  addEventListener(type: string, listener: (event: Event) => void) {
    const set = this.listeners.get(type) ?? new Set<(event: Event) => void>()
    set.add(listener)
    this.listeners.set(type, set)
  }
}

interface StreamStats {
  behaviour: Behaviour
  attempts: number
  state: StreamState
  messages: number
}

const INITIAL_STREAMS: StreamStats[] = Array.from({ length: STREAM_COUNT }, (_, index) => ({
  behaviour: index % 2 === 0 ? 'refused' : 'flapping',
  attempts: 0,
  state: 'connecting',
  messages: 0,
}))

const RUN_MS = 2_500
const MAX_REFUSALS = 5

function SseBody({ done }: { done: Done }) {
  const [stats, setStats] = useState<StreamStats[]>(INITIAL_STREAMS)
  // The live tally, read when the run ends; `stats` is only what the list renders.
  const tally = useRef<StreamStats[]>(INITIAL_STREAMS)

  useEffect(() => {
    // A fresh tally per run: StrictMode's discarded first effect run opened (and closed) sources of its own.
    tally.current = INITIAL_STREAMS
    const update = (index: number, patch: (item: StreamStats) => StreamStats) => {
      tally.current = tally.current.map((item, at) => (at === index ? patch(item) : item))
      setStats(tally.current)
    }
    const closers = INITIAL_STREAMS.map((stream, index) =>
      openEventSource(
        {
          url: `/stress/stream/${index}`,
          minBackoffMs: 40,
          maxBackoffMs: 300,
          maxRefusals: MAX_REFUSALS,
          createSource: () => new FakeEventSource(stream.behaviour, () => update(index, (item) => ({ ...item, attempts: item.attempts + 1 }))),
        },
        {
          onMessage: () => update(index, (item) => ({ ...item, messages: item.messages + 1 })),
          onStateChange: (state) => update(index, (item) => ({ ...item, state })),
        },
      ),
    )
    const timer = setTimeout(() => {
      for (const close of closers) close()
      {
        const final = tally.current
        const refused = final.filter((item) => item.behaviour === 'refused')
        const flapping = final.filter((item) => item.behaviour === 'flapping')
        const attempts = final.reduce((sum, item) => sum + item.attempts, 0)
        done(
          {
            streams: final.length,
            'total connection attempts': attempts,
            'refused: max attempts': Math.max(...refused.map((item) => item.attempts)),
            'flapping: max attempts': Math.max(...flapping.map((item) => item.attempts)),
            'flapping: messages': flapping.reduce((sum, item) => sum + item.messages, 0),
            'run ms': RUN_MS,
          },
          {
            [`refused streams give up after ${MAX_REFUSALS} attempts`]: refused.every((item) => item.attempts === MAX_REFUSALS && item.state === 'closed'),
            'flapping streams keep reconnecting with backoff': flapping.every((item) => item.attempts >= 2 && item.attempts <= 20),
            'every stream closed on cleanup': final.every((item) => item.state === 'closed'),
          },
        )
      }
    }, RUN_MS)
    return () => {
      clearTimeout(timer)
      for (const close of closers) close()
    }
    // One run per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <ul className="grid gap-x-4 gap-y-1 text-xs sm:grid-cols-2" aria-label="Streams">
      {stats.map((stream, index) => (
        <li key={index} className="flex items-center gap-2 font-mono tabular-nums">
          <span className="w-5 text-muted-foreground">#{index + 1}</span>
          <span className="w-16">{stream.behaviour}</span>
          <span className="w-24">{stream.state}</span>
          <span>{stream.attempts} attempts</span>
          {stream.messages > 0 ? <span className="text-muted-foreground">{stream.messages} msg</span> : null}
        </li>
      ))}
    </ul>
  )
}

export function SseScenario() {
  return (
    <Scenario id="sse" title={`${STREAM_COUNT} SSE connections reconnecting`} description="Five are refused (give up after five tries), five flap (reconnect with jittered backoff); all through the real wrapper on a fake transport.">
      {(done) => <SseBody done={done} />}
    </Scenario>
  )
}

const BURST_RATE_PER_S = 100
const BURST_MS = 3_000
/** A frame that took longer than this dropped at least two 60 Hz frames. */
const DROPPED_FRAME_MS = 50

function CoalescerBody({ done }: { done: Done }) {
  const queryClient = useQueryClient()
  const [metrics, setMetrics] = useState<Record<string, number | string | boolean> | null>(null)

  useEffect(() => {
    let hidden = false
    let flushes = 0
    const flushTimes: number[] = []
    const coalescer = createInvalidationCoalescer({
      isHidden: () => hidden,
      onFlush: (families) => {
        flushes += 1
        flushTimes.push(performance.now())
        for (const family of families) queryClient.invalidateQueries({ queryKey: family })
      },
    })

    // Watch the frame rate while the burst runs.
    let frames = 0
    let dropped = 0
    let longest = 0
    let last = performance.now()
    let watching = true
    const tick = (now: number) => {
      if (!watching) return
      const gap = now - last
      last = now
      frames += 1
      longest = Math.max(longest, gap)
      if (gap > DROPPED_FRAME_MS) dropped += 1
      requestAnimationFrame(tick)
    }
    requestAnimationFrame(tick)

    // Families of their own: real ones would refetch the sidebar's live queries, and the page must stay network-silent.
    const families = [['stress', 'a'], ['stress', 'b'], ['stress', 'c'], ['stress', 'd']] as const
    let sent = 0
    const started = performance.now()
    const interval = setInterval(() => {
      // Ten events per 100 ms tick: 100 events per second, spread over families.
      for (let index = 0; index < BURST_RATE_PER_S / 10; index++) {
        coalescer.add(families[sent % families.length])
        sent += 1
      }
    }, 100)

    const finish = setTimeout(() => {
      clearInterval(interval)
      watching = false
      const elapsed = performance.now() - started
      // Flushes per second, measured over every one-second window.
      let maxPerSecond = 0
      for (let index = 0; index < flushTimes.length; index++) {
        let count = 0
        for (let other = index; other < flushTimes.length && flushTimes[other] - flushTimes[index] < 1_000; other++) count += 1
        maxPerSecond = Math.max(maxPerSecond, count)
      }

      // Hidden: events pile up without a flush; visible again: one flush.
      const beforeHidden = flushes
      hidden = true
      coalescer.add(['projects'])
      coalescer.add(['executions'])
      const flushedWhileHidden = flushes - beforeHidden
      hidden = false
      coalescer.resume()
      setTimeout(() => {
        const flushedOnResume = flushes - beforeHidden
        coalescer.dispose()
        const result = {
          'events sent': sent,
          'elapsed ms': Math.round(elapsed),
          'events per second': Math.round((sent / elapsed) * 1000),
          flushes: beforeHidden,
          'max flushes in any second': maxPerSecond,
          frames,
          [`frames longer than ${DROPPED_FRAME_MS} ms`]: dropped,
          'longest frame ms': Math.round(longest),
          'flushed while hidden': flushedWhileHidden,
          'flushed on resume': flushedOnResume,
        }
        setMetrics(result)
        done(result, {
          'at most 4 flushes per second': maxPerSecond <= 4,
          'no frame longer than 50 ms during the burst': dropped === 0,
          'nothing flushed while hidden': flushedWhileHidden === 0,
          'one flush on resume': flushedOnResume === 1,
        })
      }, 300)
    }, BURST_MS)

    return () => {
      clearInterval(interval)
      clearTimeout(finish)
      watching = false
      coalescer.dispose()
    }
    // One run per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return metrics ? <Metrics values={metrics} /> : <p className="text-xs text-muted-foreground">Sending {BURST_RATE_PER_S} events per second for {BURST_MS / 1000} s…</p>
}

export function CoalescerScenario() {
  return (
    <Scenario id="coalescer" title={`${BURST_RATE_PER_S} platform events per second`} description="The live-events coalescer: flushes capped at four per second, frames watched, hidden-tab deferral checked.">
      {(done) => <CoalescerBody done={done} />}
    </Scenario>
  )
}
