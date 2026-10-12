import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react'

import { QueryBoundary } from '@/components/shared/feedback'
import { Frame } from '@/components/system/frame'
import { Tag } from '@/components/system/tag'
import { formatNumber } from '@/lib/intl'

export type ScenarioId =
  | 'projects'
  | 'tree'
  | 'file'
  | 'events'
  | 'findings'
  | 'diagram'
  | 'sse'
  | 'coalescer'
  | 'errors'

export type ScenarioMetrics = Record<string, number | string | boolean>
export type ScenarioChecks = Record<string, boolean>

export interface ScenarioResult {
  id: ScenarioId
  /** Milliseconds from the scenario being scheduled to its first commit. */
  mountMs: number
  /** Key figures the scenario wants on the readout and in the e2e assertions. */
  metrics: ScenarioMetrics
  /** Checks the scenario ran on itself; every one must hold. */
  checks: ScenarioChecks
}

interface ScenarioContextValue {
  /** Called once per scenario when it has rendered and measured itself. */
  report: (result: ScenarioResult) => void
  /** When the page scheduled each scenario to mount (written by the page, read in a layout effect). */
  scheduledAt: RefObject<Partial<Record<ScenarioId, number>>>
  /** What every scenario reported so far. */
  results: readonly ScenarioResult[]
}

export const ScenarioContext = createContext<ScenarioContextValue>({ report: () => undefined, scheduledAt: { current: {} }, results: [] })

export type Done = (metrics: ScenarioMetrics, checks: ScenarioChecks) => void

interface ScenarioProps {
  id: ScenarioId
  title: string
  description: string
  /** Rendered inside the frame; receives `done` to call once the scenario has its numbers. */
  children: (done: Done) => ReactNode
  padding?: 'none' | 'sm' | 'md'
  className?: string
}

const now = () => performance.now()

/**
 * One stress scenario in a frame: measures the time from being scheduled
 * to its first commit, waits for the body to call `done` with its metrics,
 * then reports both to the page. Each scenario has its own error boundary
 * so one failure never hides the others.
 */
export function Scenario({ id, title, description, children, padding = 'md', className }: ScenarioProps) {
  const { report, scheduledAt, results } = useContext(ScenarioContext)
  const mountMs = useRef<number | null>(null)
  const reported = useRef(false)
  const [draft, setDraft] = useState<{ metrics: ScenarioMetrics; checks: ScenarioChecks } | null>(null)

  useLayoutEffect(() => {
    mountMs.current ??= now() - (scheduledAt.current[id] ?? now())
    // Measured once, on the first commit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const done = useCallback<Done>((metrics, checks) => setDraft((current) => current ?? { metrics, checks }), [])

  useEffect(() => {
    if (!draft || reported.current) return
    reported.current = true
    report({ id, mountMs: Math.round(mountMs.current ?? 0), metrics: draft.metrics, checks: draft.checks })
  }, [draft, id, report])

  const result = results.find((item) => item.id === id)
  const failed = result ? Object.values(result.checks).some((check) => !check) : false

  return (
    <Frame
      title={title}
      description={description}
      padding={padding}
      className={className}
      data-scenario={id}
      data-ready={result ? 'true' : undefined}
      data-mount-ms={result?.mountMs}
      meta={
        result ? (
          <>
            <Tag tone={failed ? 'destructive' : 'success'}>{failed ? 'check failed' : 'ready'}</Tag>
            <Tag variant="outline" mono>
              {formatNumber(result.mountMs)} ms to first commit
            </Tag>
          </>
        ) : (
          <Tag tone="info" dot live>
            rendering
          </Tag>
        )
      }
    >
      <QueryBoundary compact>{children(done)}</QueryBoundary>
    </Frame>
  )
}

/** The scenario's metrics as a definition list under its body. */
export function Metrics({ values }: { values: ScenarioMetrics }) {
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-xs">
      {Object.entries(values).map(([key, value]) => (
        <div key={key} className="contents">
          <dt className="text-muted-foreground">{key}</dt>
          <dd className="font-mono tabular-nums">{typeof value === 'number' ? formatNumber(value) : String(value)}</dd>
        </div>
      ))}
    </dl>
  )
}

/** Resolves after the browser has painted and gone idle, so measurements do not include the previous scenario. */
export function afterIdle(timeoutMs = 400): Promise<void> {
  return new Promise((resolve) => {
    // A hidden document never paints: animation frames and idle callbacks
    // stall, so a timer stands in (the measurements are meaningless then anyway).
    if (document.hidden) {
      setTimeout(resolve, 50)
      return
    }
    requestAnimationFrame(() => {
      if ('requestIdleCallback' in window) {
        window.requestIdleCallback(() => resolve(), { timeout: timeoutMs })
      } else {
        setTimeout(resolve, 50)
      }
    })
  })
}

/** Milliseconds `work` kept the main thread busy. */
export function timed<T>(work: () => T): { value: T; ms: number } {
  const start = now()
  const value = work()
  return { value, ms: Math.round((now() - start) * 10) / 10 }
}
