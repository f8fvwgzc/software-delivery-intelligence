/*
 * Scenarios that render real feature components over synthetic data: the
 * projects table, the file tree, the code viewer, the event log and the
 * findings list. Each one measures how long the pure data work took and
 * counts the DOM rows the windowed list actually produced.
 */
import { useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState } from 'react'

import { CodeLines } from '@/features/code/components/file-view/code-lines'
import { FileTree } from '@/features/code/components/file-tree'
import { splitLines } from '@/features/code/lib/format'
import { buildTree } from '@/features/code/lib/tree'
import { executionKeys } from '@/features/executions/api/keys'
import { EventLog } from '@/features/executions/components/event-log'
import { ProjectsTable } from '@/features/projects/components/projects-table'
import { FindingsList } from '@/features/reviews/components/findings-list'

import { Metrics, Scenario, timed, type Done } from '../scenario'
import { syntheticEvents, syntheticFile, syntheticProjects, syntheticReview, syntheticTree } from '../synthetic'

export const PROJECT_COUNT = 500
export const TREE_ENTRIES = 60_000
export const FILE_BYTES = 1_000_000
export const EVENT_COUNT = 10_000
export const PR_FILES = 200


/** Reports once the children have committed; `measure` reads the DOM then. */
function useReportAfterCommit(done: Done, measure: (root: HTMLElement) => { metrics: Record<string, number | string | boolean>; checks: Record<string, boolean> }) {
  const rootRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    // Lists windowed against the page scroll only render what is on screen:
    // bring the scenario into view, give the virtualizer a frame to measure, then count.
    root.scrollIntoView({ block: 'start' })
    const report = () => {
      const { metrics, checks } = measure(root)
      done(metrics, checks)
    }
    // A hidden document never paints, so its frames never come; a timer stands in.
    if (document.hidden) {
      const timer = setTimeout(report, 100)
      return () => clearTimeout(timer)
    }
    let inner = 0
    const frame = requestAnimationFrame(() => {
      inner = requestAnimationFrame(report)
    })
    return () => {
      cancelAnimationFrame(frame)
      cancelAnimationFrame(inner)
    }
    // Report exactly once, from the first commit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return rootRef
}

function ProjectsBody({ done }: { done: Done }) {
  const [{ value: projects, ms }] = useState(() => timed(() => syntheticProjects(PROJECT_COUNT)))
  const rootRef = useReportAfterCommit(done, (root) => {
    const rows = root.querySelectorAll('tbody tr[data-index]').length
    return {
      metrics: { projects: projects.length, 'generate ms': ms, 'rows in DOM': rows },
      checks: { 'windowed (rows in DOM < 100)': rows > 0 && rows < 100 },
    }
  })
  return (
    <div ref={rootRef} className="flex flex-col gap-3">
      <ProjectsTable projects={projects} />
    </div>
  )
}

export function ProjectsScenario() {
  return (
    <Scenario id="projects" title={`Workspace with ${PROJECT_COUNT} projects`} description="The real projects table; rows are windowed against the page scroll." padding="none">
      {(done) => <ProjectsBody done={done} />}
    </Scenario>
  )
}

function TreeBody({ done }: { done: Done }) {
  const [{ value: model, ms, entries }] = useState(() => {
    const generated = timed(() => syntheticTree(TREE_ENTRIES))
    const built = timed(() => buildTree(generated.value))
    return { value: built.value, ms: built.ms, entries: generated.value.entries.length }
  })
  const rootRef = useReportAfterCommit(done, (root) => {
    const rows = root.querySelectorAll('[role="treeitem"]').length
    return {
      metrics: { entries, 'buildTree ms': ms, 'rows in DOM': rows },
      checks: { 'windowed (rows in DOM < 200)': rows > 0 && rows < 200, 'buildTree < 100 ms': ms < 100 },
    }
  })
  return (
    <div ref={rootRef} className="flex h-80 flex-col rounded-lg p-2 ring-1 ring-border">
      <FileTree model={model} projectId="stress-project" currentPath="" className="flex min-h-0 flex-1 flex-col gap-2" />
    </div>
  )
}

export function TreeScenario() {
  return (
    <Scenario id="tree" title={`Repository tree with ${TREE_ENTRIES.toLocaleString()} entries`} description="The real file tree: built once, filtered, keyboard navigable, windowed.">
      {(done) => <TreeBody done={done} />}
    </Scenario>
  )
}

function FileBody({ done }: { done: Done }) {
  const [{ value: lines, ms, bytes }] = useState(() => {
    const generated = timed(() => syntheticFile(FILE_BYTES))
    const split = timed(() => splitLines(generated.value))
    return { value: split.value, ms: generated.ms + split.ms, bytes: generated.value.length }
  })
  const rootRef = useReportAfterCommit(done, (root) => {
    const rows = root.querySelectorAll('pre > div').length
    return {
      metrics: { bytes, lines: lines.length, 'generate+split ms': ms, 'rows in DOM': rows },
      checks: { 'windowed (rows in DOM < 200)': rows > 0 && rows < 200 },
    }
  })
  return (
    <div ref={rootRef} className="rounded-lg ring-1 ring-border">
      <CodeLines lines={lines} highlighted={null} />
    </div>
  )
}

export function FileScenario() {
  return (
    <Scenario id="file" title="A 1 MB file in the code viewer" description="Plain text (files past 200 KB are never tokenised); lines windowed against the page scroll." padding="none">
      {(done) => <FileBody done={done} />}
    </Scenario>
  )
}

function EventsBody({ done }: { done: Done }) {
  const queryClient = useQueryClient()
  const [executionId] = useState(() => `stress-execution-${Math.floor(Math.random() * 1e6)}`)
  // Seeded into the cache before the log mounts, so its suspense query resolves at once and nothing is fetched.
  const [{ ms, total }] = useState(() => {
    const generated = timed(() => syntheticEvents(EVENT_COUNT, executionId))
    queryClient.setQueryData(executionKeys.events(executionId), generated.value)
    return { ms: generated.ms, total: generated.value.length }
  })
  const rootRef = useReportAfterCommit(done, (root) => {
    const rows = root.querySelectorAll('ol > li').length
    return {
      metrics: { events: total, 'generate ms': ms, 'rows in DOM': rows },
      checks: { 'windowed (rows in DOM < 120)': rows > 0 && rows < 120 },
    }
  })
  return (
    <div ref={rootRef}>
      <EventLog executionId={executionId} status="succeeded" steps={[]} />
    </div>
  )
}

export function EventsScenario() {
  return (
    <Scenario id="events" title={`Execution with ${EVENT_COUNT.toLocaleString()} events`} description="The real event log over a seeded cache; the timeline is windowed inside its own scroll pane.">
      {(done) => <EventsBody done={done} />}
    </Scenario>
  )
}

function FindingsBody({ done }: { done: Done }) {
  const [{ value: review, ms }] = useState(() => timed(() => syntheticReview(PR_FILES)))
  const rootRef = useReportAfterCommit(done, (root) => {
    const rows = root.querySelectorAll('ol > li').length
    return {
      metrics: { files: review.filesReviewed, findings: review.findingsTotal, 'generate ms': ms, 'rows in DOM': rows },
      checks: { 'windowed (rows in DOM < 100)': rows > 0 && rows < 100 },
    }
  })
  return (
    <div ref={rootRef}>
      <FindingsList review={review} />
    </div>
  )
}

export function FindingsScenario() {
  return (
    <Scenario id="findings" title={`Pull request review of ${PR_FILES} files`} description="The real findings list, grouped by severity; rows are windowed and measured (markdown inside)." padding="none">
      {(done) => <FindingsBody done={done} />}
    </Scenario>
  )
}

/** Shared across scenarios that want the numbers under the body. */
export function ScenarioMetrics({ values }: { values: Record<string, number | string | boolean> }) {
  const stable = useMemo(() => values, [values])
  return <Metrics values={stable} />
}
