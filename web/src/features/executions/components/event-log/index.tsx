import { useQueryClient, useSuspenseQuery } from '@tanstack/react-query'
import { ArrowDownToLineIcon } from 'lucide-react'
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
import { mergeEvents, useEventStream, type StreamState } from './use-event-stream'

export { EventLogSkeleton } from './skeleton'

interface EventLogProps {
  executionId: string
  status: components['schemas']['ExecutionStatus']
  steps: components['schemas']['Step'][]
}

const ALL_STEPS = '__all'
const INVALIDATE_DEBOUNCE = 250
const FOLLOW_THRESHOLD = 24

const STREAM_COPY: Record<StreamState, { label: string; dot: string; live?: boolean }> = {
  idle: { label: 'idle', dot: 'bg-muted-foreground/50' },
  connecting: { label: 'connecting', dot: 'bg-muted-foreground/50' },
  live: { label: 'live', dot: 'bg-success', live: true },
  reconnecting: { label: 'reconnecting', dot: 'bg-warning', live: true },
  ended: { label: 'ended', dot: 'bg-muted-foreground/50' },
}

/** Dense mono timeline of the execution's events, tailed over SSE. */
export function EventLog({ executionId, status, steps }: EventLogProps) {
  const queryClient = useQueryClient()
  const { data: initial } = useSuspenseQuery(executionQueries.events(executionId))
  const [stepFilter, setStepFilter] = useState(ALL_STEPS)
  const [follow, setFollow] = useState(true)
  const viewportRef = useRef<HTMLDivElement>(null)
  const invalidateTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  const afterSeq = initial.length > 0 ? initial[initial.length - 1].seq : 0

  // Every streamed event means the execution changed; refresh it, coalescing bursts.
  const scheduleInvalidate = () => {
    if (invalidateTimer.current) clearTimeout(invalidateTimer.current)
    invalidateTimer.current = setTimeout(() => {
      queryClient.invalidateQueries({ queryKey: executionKeys.detail(executionId) })
      queryClient.invalidateQueries({ queryKey: executionKeys.lists() })
      queryClient.invalidateQueries({ queryKey: approvalKeys.all })
    }, INVALIDATE_DEBOUNCE)
  }

  useEffect(() => {
    return () => {
      if (invalidateTimer.current) clearTimeout(invalidateTimer.current)
    }
  }, [])

  const { events: streamed, state } = useEventStream({
    executionId,
    afterSeq,
    enabled: !isTerminal(status),
    onEvent: scheduleInvalidate,
    onEnd: scheduleInvalidate,
  })

  const events = useMemo(() => mergeEvents(initial, streamed), [initial, streamed])
  const visible = stepFilter === ALL_STEPS ? events : events.filter((event) => event.stepKey === stepFilter)
  const visibleCount = visible.length

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
              <span className={cn('absolute inline-flex size-full animate-ping rounded-full opacity-60', stream.dot)} />
            )}
            <span className={cn('relative inline-flex size-1.5 rounded-full', stream.dot)} />
          </span>
          {stream.label}
          <span className="tabular-nums">· {events.length}</span>
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
        className="h-[32rem] overflow-y-auto bg-muted/20"
        style={{ scrollbarGutter: 'stable' }}
      >
        {visible.length === 0 ? (
          <p className="px-3 py-6 text-center text-xs text-muted-foreground">
            {events.length === 0 ? 'No events yet. Start the execution to see them arrive.' : 'No events for this step.'}
          </p>
        ) : (
          <ol className="flex flex-col py-1">
            {visible.map((event) => (
              <EventRow key={event.seq} event={event} />
            ))}
          </ol>
        )}
      </div>
    </section>
  )
}
