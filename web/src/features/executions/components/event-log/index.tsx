import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { ArrowDownToLineIcon, TerminalIcon } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { approvalKeys } from '@/features/approvals/api/keys'
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Toggle } from '@/components/ui/toggle'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { executionKeys } from '../../api/keys'
import { executionQueries } from '../../api/queries'
import { isTerminal } from '../../lib/labels'
import { EventRow } from './event-row'
import { LogTerminal } from './log-terminal'
import { appendEvent, useEventStream, type ExecutionEvent, type StreamState } from './use-event-stream'

export { EventLogSkeleton } from './skeleton'

interface EventLogProps {
  executionId: string
  status: components['schemas']['ExecutionStatus']
  steps: components['schemas']['Step'][]
}

const ALL_STEPS = '__all'
const INVALIDATE_DEBOUNCE = 250
const FOLLOW_THRESHOLD = 24
/**
 * Rows rendered while following a long run. The list is not virtualised, so
 * a run that streams thousands of adapter lines would otherwise reconcile
 * every row per line; the newest window is what an operator watches, and
 * the rest is one click away.
 */
const TAIL_WINDOW = 300

const STREAM_COPY: Record<StreamState, { label: string; dot: string; live?: boolean }> = {
  idle: { label: 'idle', dot: 'bg-muted-foreground/50' },
  connecting: { label: 'connecting', dot: 'bg-muted-foreground/50' },
  live: { label: 'live', dot: 'bg-success', live: true },
  reconnecting: { label: 'reconnecting', dot: 'bg-warning', live: true },
  unavailable: { label: 'stream unavailable, polling', dot: 'bg-warning' },
  ended: { label: 'ended', dot: 'bg-muted-foreground/50' },
}

/** The newest `TAIL_WINDOW` rows unless the reader asked for all of them. */
function tail<T>(rows: T[], showAll: boolean) {
  const hidden = showAll ? 0 : Math.max(0, rows.length - TAIL_WINDOW)
  return { rows: hidden > 0 ? rows.slice(hidden) : rows, hidden }
}

/**
 * Dense mono timeline of the execution's structured events, tailed over SSE,
 * with the adapter's `log` lines in a separate terminal block underneath.
 */
export function EventLog({ executionId, status, steps }: EventLogProps) {
  const queryClient = useQueryClient()
  const { data: initial } = useSuspenseQuery(executionQueries.events(executionId))
  const [stepFilter, setStepFilter] = useState(ALL_STEPS)
  const [follow, setFollow] = useState(true)
  const [showLogs, setShowLogs] = useState(false)
  const [followLogs, setFollowLogs] = useState(true)
  const [showAll, setShowAll] = useState(false)
  const viewportRef = useRef<HTMLDivElement>(null)
  const invalidateTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  // Resume point for the stream, fixed at mount: streamed events are
  // appended to the cache, which is the one list everything (this log, the
  // rail) reads, and that must not reconnect the stream on every line.
  const [afterSeq] = useState(() => (initial.length > 0 ? initial[initial.length - 1].seq : 0))

  // Every streamed event means the execution changed; refresh it, coalescing bursts.
  const scheduleInvalidate = () => {
    if (invalidateTimer.current) clearTimeout(invalidateTimer.current)
    invalidateTimer.current = setTimeout(() => {
      queryClient.invalidateQueries({ queryKey: executionKeys.detail(executionId) })
      queryClient.invalidateQueries({ queryKey: executionKeys.lists() })
      queryClient.invalidateQueries({ queryKey: approvalKeys.all })
    }, INVALIDATE_DEBOUNCE)
  }

  const appendToCache = (event: ExecutionEvent) => {
    queryClient.setQueryData<ExecutionEvent[]>(executionKeys.events(executionId), (cached = []) =>
      appendEvent(cached, event),
    )
  }

  useEffect(() => {
    return () => {
      if (invalidateTimer.current) clearTimeout(invalidateTimer.current)
    }
  }, [])

  const { state } = useEventStream({
    executionId,
    afterSeq,
    enabled: !isTerminal(status),
    onEvent: (event) => {
      appendToCache(event)
      scheduleInvalidate()
    },
    onEnd: scheduleInvalidate,
  })

  const { timeline, logs } = useMemo(() => {
    const timeline: ExecutionEvent[] = []
    const logs: ExecutionEvent[] = []
    for (const event of initial) (event.type === 'log' ? logs : timeline).push(event)
    return { timeline, logs }
  }, [initial])

  const visible = stepFilter === ALL_STEPS ? timeline : timeline.filter((event) => event.stepKey === stepFilter)
  const visibleLogs = stepFilter === ALL_STEPS ? logs : logs.filter((event) => event.stepKey === stepFilter)
  const visibleCount = visible.length
  const shown = tail(visible, showAll)
  const shownLogs = tail(visibleLogs, showAll)

  // Keep the newest line in view while following. Scroll position is DOM state, not React state.
  useEffect(() => {
    const viewport = viewportRef.current
    if (!follow || visibleCount === 0 || !viewport) return
    viewport.scrollTop = viewport.scrollHeight
  }, [follow, visibleCount])

  const handleScroll = () => {
    const viewport = viewportRef.current
    if (!viewport || !follow) return
    const distanceFromBottom = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight
    if (distanceFromBottom > FOLLOW_THRESHOLD) setFollow(false)
  }

  const stream = STREAM_COPY[state]

  return (
    <section className="flex flex-col overflow-hidden rounded-lg ring-1 ring-border" aria-label="Event log">
      <header className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
        <h2 className="text-sm font-medium">Events</h2>
        <span className="inline-flex items-center gap-1.5 text-2xs text-muted-foreground">
          <span className="relative flex size-1.5">
            {stream.live && (
              <span className={cn('absolute inline-flex size-full rounded-full opacity-60 motion-safe:animate-ping', stream.dot)} />
            )}
            <span className={cn('relative inline-flex size-1.5 rounded-full', stream.dot)} />
          </span>
          {stream.label}
          <span className="tabular-nums">· {timeline.length}</span>
        </span>
        <Select value={stepFilter} onValueChange={setStepFilter}>
          <SelectTrigger size="sm" className="ml-auto max-w-36 text-xs" aria-label="Filter by step">
            <SelectValue />
          </SelectTrigger>
          <SelectContent align="end">
            <SelectGroup>
              <SelectItem value={ALL_STEPS}>All steps</SelectItem>
              {steps.map((step) => (
                <SelectItem key={step.key} value={step.key} className="font-mono text-xs">
                  {step.key}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
        <Toggle size="sm" variant="outline" pressed={follow} onPressedChange={setFollow} aria-label="Follow new events" className="text-xs">
          <ArrowDownToLineIcon data-icon="inline-start" />
          Follow
        </Toggle>
      </header>
      <div
        ref={viewportRef}
        onScroll={handleScroll}
        className={cn('overflow-y-auto bg-muted/20', showLogs ? 'h-[20rem]' : 'h-[32rem]')}
        style={{ scrollbarGutter: 'stable' }}
      >
        {visible.length === 0 ? (
          <p className="px-3 py-6 text-center text-xs text-muted-foreground">
            {timeline.length === 0 ? 'No events yet. Start the execution to see them arrive.' : 'No events for this step.'}
          </p>
        ) : (
          <ol className="flex flex-col py-1">
            {shown.hidden > 0 ? (
              <li className="px-3 py-1">
                <button
                  type="button"
                  onClick={() => setShowAll(true)}
                  className="rounded-sm text-xs text-muted-foreground underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
                >
                  Show {shown.hidden} earlier {shown.hidden === 1 ? 'event' : 'events'}
                </button>
              </li>
            ) : null}
            {shown.rows.map((event) => (
              <EventRow key={event.seq} event={event} />
            ))}
          </ol>
        )}
      </div>
      <div className="flex h-9 shrink-0 items-center gap-2 border-t px-3">
        <Toggle
          size="sm"
          variant="outline"
          pressed={showLogs}
          onPressedChange={setShowLogs}
          aria-label="Show adapter logs"
          aria-controls={showLogs ? `execution-logs-${executionId}` : undefined}
          className="text-xs"
        >
          <TerminalIcon data-icon="inline-start" />
          Show logs
        </Toggle>
        <span className="text-2xs tabular-nums text-muted-foreground">
          {visibleLogs.length} {visibleLogs.length === 1 ? 'line' : 'lines'}
        </span>
        {showLogs ? (
          <Toggle
            size="sm"
            variant="outline"
            pressed={followLogs}
            onPressedChange={setFollowLogs}
            aria-label="Follow new log lines"
            className="ml-auto text-xs"
          >
            <ArrowDownToLineIcon data-icon="inline-start" />
            Follow
          </Toggle>
        ) : null}
      </div>
      {showLogs ? (
        <div id={`execution-logs-${executionId}`} className="border-t">
          <LogTerminal
            lines={shownLogs.rows}
            hidden={shownLogs.hidden}
            onShowAll={() => setShowAll(true)}
            follow={followLogs}
            onFollowChange={setFollowLogs}
          />
        </div>
      ) : null}
    </section>
  )
}
