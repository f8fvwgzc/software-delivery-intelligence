import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { ArrowDownToLineIcon, TerminalIcon } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { ComboboxField } from '@/components/shared/form'
import { useScrollRows } from '@/components/system/virtual-rows'
import { Toggle } from '@/components/ui/toggle'
import { approvalKeys } from '@/features/approvals/api/keys'
import type { components } from '@/lib/api/schema'
import { count } from '@/lib/intl'
import { cn } from '@/lib/utils'

import { executionKeys } from '../../api/keys'
import { executionQueries } from '../../api/queries'
import { isTerminal } from '../../lib/labels'
import { EventRow, EVENT_ROW_HEIGHT } from './event-row'
import { LogTerminal } from './log-terminal'
import { appendEvents, useEventStream, type ExecutionEvent, type StreamState } from './use-event-stream'

export { EventLogSkeleton } from './skeleton'

interface EventLogProps {
  executionId: string
  status: components['schemas']['ExecutionStatus']
  steps: components['schemas']['Step'][]
}

const ALL_STEPS = '__all'
const INVALIDATE_DEBOUNCE = 250
/** Streamed events are appended to the cache in one batch per animation frame, so a burst costs one render. */
const APPEND_FLUSH_MS = 32
const FOLLOW_THRESHOLD = 24
/**
 * Adapter log lines rendered while following a long run. The terminal is
 * not windowed (its lines wrap), so the newest window is what an operator
 * watches and the rest is one click away. The timeline above it is
 * windowed and shows every event.
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
 * The timeline is windowed: ten thousand events keep a few dozen rows in
 * the DOM, and a burst of streamed events lands in the cache once per frame.
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
  const pendingRef = useRef<ExecutionEvent[]>([])
  const appendTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

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

  const flushPending = () => {
    appendTimer.current = undefined
    const batch = pendingRef.current
    if (batch.length === 0) return
    pendingRef.current = []
    queryClient.setQueryData<ExecutionEvent[]>(executionKeys.events(executionId), (cached = []) => appendEvents(cached, batch))
  }

  const queueAppend = (event: ExecutionEvent) => {
    pendingRef.current.push(event)
    appendTimer.current ??= setTimeout(flushPending, APPEND_FLUSH_MS)
  }

  useEffect(() => {
    return () => {
      if (invalidateTimer.current) clearTimeout(invalidateTimer.current)
      if (appendTimer.current) clearTimeout(appendTimer.current)
    }
  }, [])

  const { state } = useEventStream({
    executionId,
    afterSeq,
    enabled: !isTerminal(status),
    onEvent: (event) => {
      queueAppend(event)
      scheduleInvalidate()
    },
    onEnd: () => {
      flushPending()
      scheduleInvalidate()
    },
  })

  const { timeline, logs } = useMemo(() => {
    const timeline: ExecutionEvent[] = []
    const logs: ExecutionEvent[] = []
    for (const event of initial) (event.type === 'log' ? logs : timeline).push(event)
    return { timeline, logs }
  }, [initial])

  const visible = useMemo(
    () => (stepFilter === ALL_STEPS ? timeline : timeline.filter((event) => event.stepKey === stepFilter)),
    [timeline, stepFilter],
  )
  const visibleLogs = stepFilter === ALL_STEPS ? logs : logs.filter((event) => event.stepKey === stepFilter)
  const visibleCount = visible.length
  const shownLogs = tail(visibleLogs, showAll)

  const rows = useScrollRows({ count: visibleCount, estimateSize: EVENT_ROW_HEIGHT, scrollRef: viewportRef })
  const { scrollToIndex } = rows

  // Keep the newest line in view while following. Scroll position is DOM state, not React state.
  useEffect(() => {
    if (!follow || visibleCount === 0) return
    scrollToIndex(visibleCount - 1, { align: 'end' })
  }, [follow, visibleCount, scrollToIndex])

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
        <ComboboxField
          size="sm"
          align="end"
          className="ml-auto w-36"
          aria-label="Filter by step"
          value={stepFilter}
          onValueChange={setStepFilter}
          options={[
            { value: ALL_STEPS, label: 'All steps' },
            ...steps.map((step) => ({ value: step.key, label: step.key, description: step.title })),
          ]}
          searchPlaceholder="Search steps"
          emptyText="No step matches."
        />
        <Toggle size="sm" variant="outline" pressed={follow} onPressedChange={setFollow} aria-label="Follow new events" className="text-xs">
          <ArrowDownToLineIcon data-icon="inline-start" />
          Follow
        </Toggle>
      </header>
      <div
        ref={viewportRef}
        onScroll={handleScroll}
        role="region"
        aria-label="Event timeline"
        tabIndex={0}
        className={cn('overflow-y-auto bg-muted/20 outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset', showLogs ? 'h-[20rem]' : 'h-[32rem]')}
        style={{ scrollbarGutter: 'stable' }}
        data-virtual={rows.virtual || undefined}
      >
        {visible.length === 0 ? (
          <p className="px-3 py-6 text-center text-xs text-muted-foreground">
            {timeline.length === 0 ? 'No events yet. Start the execution to see them arrive.' : 'No events for this step.'}
          </p>
        ) : (
          <ol
            className={cn('flex flex-col py-1', rows.virtual && 'relative')}
            style={rows.virtual ? { height: rows.totalSize } : undefined}
          >
            {rows.items.map((item) => {
              const event = visible[item.index]
              if (!event) return null
              return (
                <EventRow
                  key={event.seq}
                  event={event}
                  index={item.index}
                  measure={rows.virtual ? rows.measure : undefined}
                  style={rows.virtual ? { position: 'absolute', top: 0, left: 0, right: 0, transform: `translateY(${item.start}px)` } : undefined}
                />
              )
            })}
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
        <span className="text-2xs tabular-nums text-muted-foreground">{count(visibleLogs.length, 'line', 'lines')}</span>
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
