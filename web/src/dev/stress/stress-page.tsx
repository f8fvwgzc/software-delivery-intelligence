/*
 * /dev/stress — development only (the router adds the route behind
 * `import.meta.env.DEV`, so this directory never reaches a production
 * bundle). Every worst case the frontend must survive, rendered over
 * synthetic data generated at runtime, one scenario after another so each
 * one's cost shows on its own. The readout at the top collects the long
 * tasks the main thread recorded and the per-navigation request budget;
 * `e2e/stress.spec.ts` reads the same numbers from `window.sdiDevPerf` and
 * the `data-*` attributes.
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ComponentType, type RefObject } from 'react'

import { Page, PageHeader } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Tag } from '@/components/system/tag'
import { Button } from '@/components/ui/button'
import { formatNumber } from '@/lib/intl'
import { LONG_TASK_WARN_MS, type LongTask } from '@/lib/perf/dev-perf'

import { afterIdle, Metrics, ScenarioContext, type ScenarioId, type ScenarioResult } from './scenario'
import { EventsScenario, FileScenario, FindingsScenario, ProjectsScenario, TreeScenario } from './scenarios/data-scenarios'
import { DiagramScenario } from './scenarios/diagram-scenario'
import { ErrorsScenario } from './scenarios/error-scenarios'
import { CoalescerScenario, SseScenario } from './scenarios/realtime-scenarios'

/** Long tasks over this fail the stress spec; the dev warning threshold is lower. */
export const LONG_TASK_FAIL_MS = 200

const SCENARIOS: { id: ScenarioId; Component: ComponentType }[] = [
  { id: 'projects', Component: ProjectsScenario },
  { id: 'tree', Component: TreeScenario },
  { id: 'file', Component: FileScenario },
  { id: 'events', Component: EventsScenario },
  { id: 'findings', Component: FindingsScenario },
  { id: 'diagram', Component: DiagramScenario },
  { id: 'sse', Component: SseScenario },
  { id: 'coalescer', Component: CoalescerScenario },
  { id: 'errors', Component: ErrorsScenario },
]

/** Long tasks recorded since `startedAt`, re-read twice a second while the page runs. */
function useLongTasks(startedAt: RefObject<number>) {
  const [tasks, setTasks] = useState<LongTask[]>([])
  useEffect(() => {
    const read = () => setTasks((window.sdiDevPerf?.longTasks ?? []).filter((task) => task.startTime >= startedAt.current))
    const interval = setInterval(read, 500)
    return () => clearInterval(interval)
  }, [startedAt])
  return tasks
}

function Readout({ results, mounted, total, startedAt }: { results: ScenarioResult[]; mounted: number; total: number; startedAt: RefObject<number> }) {
  const since = useLongTasks(startedAt)
  const over = since.filter((task) => task.duration > LONG_TASK_FAIL_MS)
  const warn = since.filter((task) => task.duration > LONG_TASK_WARN_MS)
  const failedChecks = results.flatMap((result) => Object.entries(result.checks).filter(([, ok]) => !ok).map(([name]) => `${result.id}: ${name}`))
  const navigation = window.sdiDevPerf?.navigations.at(-1)
  const done = results.length === total

  const values = useMemo(
    () => ({
      'scenarios mounted': `${mounted} / ${total}`,
      'scenarios reported': `${results.length} / ${total}`,
      [`long tasks > ${LONG_TASK_WARN_MS} ms`]: warn.length,
      [`long tasks > ${LONG_TASK_FAIL_MS} ms`]: over.length,
      'longest task ms': since.reduce((max, task) => Math.max(max, task.duration), 0),
      'requests on first paint': navigation?.urls.length ?? 0,
      'checks failed': failedChecks.length,
    }),
    [mounted, total, results.length, warn.length, over.length, since, navigation, failedChecks.length],
  )

  return (
    <Frame
      title="Readout"
      description="Long tasks from PerformanceObserver since this page mounted; requests counted by the dev client middleware."
      meta={
        done ? (
          <Tag tone={over.length === 0 && failedChecks.length === 0 ? 'success' : 'destructive'}>
            {over.length === 0 && failedChecks.length === 0 ? 'within budget' : 'over budget'}
          </Tag>
        ) : (
          <Tag tone="info" dot live>
            running
          </Tag>
        )
      }
      data-stress-readout
      data-long-tasks-over={over.length}
      data-checks-failed={failedChecks.length}
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <Metrics values={values} />
        <div className="flex flex-col gap-1 text-xs">
          {failedChecks.length > 0 ? (
            <ul className="flex flex-col gap-0.5 text-destructive" aria-label="Failed checks">
              {failedChecks.map((check) => (
                <li key={check}>{check}</li>
              ))}
            </ul>
          ) : null}
          {warn.length > 0 ? (
            <ol className="flex flex-col gap-0.5 font-mono tabular-nums text-muted-foreground" aria-label="Long tasks">
              {warn.slice(-8).map((task, index) => (
                <li key={`${task.startTime}-${index}`}>
                  {formatNumber(task.duration)} ms at {formatNumber(task.startTime)} ms
                </li>
              ))}
            </ol>
          ) : (
            <p className="text-muted-foreground">No task over {LONG_TASK_WARN_MS} ms recorded.</p>
          )}
        </div>
      </div>
    </Frame>
  )
}

export default function StressPage() {
  const startedAt = useRef(0)
  const [mounted, setMounted] = useState(0)
  const [results, setResults] = useState<ScenarioResult[]>([])
  const [run, setRun] = useState(0)
  const scheduledAt = useRef<Partial<Record<ScenarioId, number>>>({})

  // The first scenario is scheduled like every other one: after the page has
  // painted and the browser is idle. `run` is the restart counter: a new run
  // schedules the first scenario again.
  useEffect(() => {
    let active = true
    startedAt.current ||= performance.now()
    void afterIdle().then(() => {
      if (!active) return
      scheduledAt.current[SCENARIOS[0].id] = performance.now()
      setMounted((count) => Math.max(count, 1))
    })
    return () => {
      active = false
    }
    // oxlint-disable-next-line react/exhaustive-effect-dependencies
  }, [run])

  // The next scenario mounts once the previous one reported and the browser is idle again.
  const report = useCallback((result: ScenarioResult) => {
    setResults((current) => (current.some((item) => item.id === result.id) ? current : [...current, result]))
    const nextIndex = SCENARIOS.findIndex((item) => item.id === result.id) + 1
    const next = SCENARIOS[nextIndex]
    if (!next) return
    void afterIdle().then(() => {
      scheduledAt.current[next.id] = performance.now()
      setMounted((count) => Math.max(count, nextIndex + 1))
    })
  }, [])

  const context = useMemo(() => ({ report, scheduledAt, results }), [report, results])
  const done = results.length === SCENARIOS.length

  const restart = () => {
    window.sdiDevPerf?.reset()
    scheduledAt.current = {}
    startedAt.current = performance.now()
    setResults([])
    setMounted(0)
    setRun((value) => value + 1)
  }

  return (
    <Page data-stress-ready={done ? 'true' : undefined} data-stress-run={run}>
      <PageHeader
        title="Stress"
        description="Worst cases over synthetic data: every scenario must render progressively, keep the main thread under 100 ms per task and offer a way out."
        actions={
          <Button variant="outline" size="sm" onClick={restart} disabled={!done}>
            Run again
          </Button>
        }
      />
      <Readout results={results} mounted={mounted} total={SCENARIOS.length} startedAt={startedAt} />
      <ScenarioContext.Provider value={context}>
        {SCENARIOS.slice(0, mounted).map(({ id, Component }) => (
          <Component key={`${id}-${run}`} />
        ))}
      </ScenarioContext.Provider>
    </Page>
  )
}
