/*
 * Development-only instrumentation (never bundled into production: it is
 * imported behind `import.meta.env.DEV` from main.tsx).
 *
 * - Long tasks: a PerformanceObserver on `longtask` keeps every task the
 *   main thread spent more than 50 ms on and warns above `LONG_TASK_WARN_MS`.
 * - Request budget: the API client reports every request it starts; the
 *   shell marks every navigation. A page that issues more than
 *   `FIRST_PAINT_BUDGET` requests within `FIRST_PAINT_WINDOW_MS` of its
 *   navigation is logged (CONTRACT.md §21: ≤ 4 requests on first paint).
 *
 * The stress page (`src/dev/stress`) and `e2e/stress.spec.ts` read
 * `window.sdiDevPerf` to assert the budgets.
 */

export interface LongTask {
  startTime: number
  duration: number
  name: string
}

export interface NavigationRequests {
  pathname: string
  startedAt: number
  /** Request URLs started within the first-paint window, in order. */
  urls: string[]
}

export interface DevPerf {
  longTasks: LongTask[]
  navigations: NavigationRequests[]
  markNavigation: (pathname: string) => void
  recordRequest: (url: string) => void
  /** Drops every recorded task, so a measurement can start from zero. */
  reset: () => void
}

export const LONG_TASK_WARN_MS = 100
export const FIRST_PAINT_BUDGET = 4
export const FIRST_PAINT_WINDOW_MS = 1_500

export function installDevPerf(): DevPerf {
  if (window.sdiDevPerf) return window.sdiDevPerf

  const longTasks: LongTask[] = []
  const navigations: NavigationRequests[] = []
  let current: NavigationRequests | null = null
  let reportTimer: ReturnType<typeof setTimeout> | undefined

  if (typeof PerformanceObserver !== 'undefined' && PerformanceObserver.supportedEntryTypes?.includes('longtask')) {
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        const task = { startTime: Math.round(entry.startTime), duration: Math.round(entry.duration), name: entry.name }
        longTasks.push(task)
        if (task.duration > LONG_TASK_WARN_MS) {
          console.warn(`[perf] long task ${task.duration} ms at ${task.startTime} ms`)
        }
      }
    })
    observer.observe({ type: 'longtask', buffered: true })
  }

  const report = () => {
    if (!current) return
    const { pathname, urls } = current
    if (urls.length > FIRST_PAINT_BUDGET) {
      console.warn(`[perf] ${pathname} issued ${urls.length} requests on first paint (budget ${FIRST_PAINT_BUDGET}):\n  ${urls.join('\n  ')}`)
    }
  }

  const perf: DevPerf = {
    longTasks,
    navigations,
    markNavigation(pathname) {
      if (reportTimer) clearTimeout(reportTimer)
      current = { pathname, startedAt: performance.now(), urls: [] }
      navigations.push(current)
      reportTimer = setTimeout(report, FIRST_PAINT_WINDOW_MS)
    },
    recordRequest(url) {
      if (!current) return
      if (performance.now() - current.startedAt > FIRST_PAINT_WINDOW_MS) return
      const parsed = new URL(url, window.location.origin)
      current.urls.push(parsed.pathname + parsed.search)
    },
    reset() {
      longTasks.length = 0
    },
  }
  window.sdiDevPerf = perf
  return perf
}
