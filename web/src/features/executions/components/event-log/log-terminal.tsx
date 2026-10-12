import { useEffect, useRef } from 'react'

import { count as countOf, formatClock } from '@/lib/intl'

import type { ExecutionEvent } from './use-event-stream'

interface LogTerminalProps {
  /** Only `log` events, in sequence order. */
  lines: ExecutionEvent[]
  /** Older lines left out of `lines`; a click brings them back. */
  hidden?: number
  onShowAll?: () => void
  /** Scrolls to the newest line as lines arrive; the user scrolling up turns it off. */
  follow: boolean
  onFollowChange: (follow: boolean) => void
}

const FOLLOW_THRESHOLD = 24

/**
 * The adapter's stdout/stderr as a terminal-style block: a muted dark-ish
 * surface, mono, long lines wrapped, auto-follow. Separate from the
 * structured timeline so the narrative stays readable.
 */
export function LogTerminal({ lines, hidden = 0, onShowAll, follow, onFollowChange }: LogTerminalProps) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const count = lines.length

  // Scroll position is DOM state; the only React input is the line count.
  useEffect(() => {
    const viewport = viewportRef.current
    if (!follow || count === 0 || !viewport) return
    viewport.scrollTop = viewport.scrollHeight
  }, [follow, count])

  const handleScroll = () => {
    const viewport = viewportRef.current
    if (!viewport || !follow) return
    const distanceFromBottom = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight
    if (distanceFromBottom > FOLLOW_THRESHOLD) onFollowChange(false)
  }

  return (
    <div
      ref={viewportRef}
      onScroll={handleScroll}
      role="log"
      aria-label="Adapter output"
      className="max-h-72 overflow-y-auto bg-muted px-3 py-2 font-mono text-xs leading-5"
      style={{ scrollbarGutter: 'stable' }}
      tabIndex={0}
    >
      {count === 0 ? (
        <p className="py-4 text-center text-muted-foreground">No adapter output yet.</p>
      ) : (
        <ol className="flex flex-col">
          {hidden > 0 ? (
            <li>
              <button
                type="button"
                onClick={onShowAll}
                className="rounded-sm text-muted-foreground underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                Show {countOf(hidden, 'earlier line', 'earlier lines')}
              </button>
            </li>
          ) : null}
          {lines.map((line) => (
            <li key={line.seq} className="grid grid-cols-[3.75rem_minmax(0,1fr)] gap-x-2">
              <span className="tabular-nums text-muted-foreground select-none">{formatClock(line.at)}</span>
              <span className="min-w-0 whitespace-pre-wrap break-all text-foreground/90">
                {line.stepKey ? <span className="text-muted-foreground">[{line.stepKey}] </span> : null}
                {line.message}
              </span>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
