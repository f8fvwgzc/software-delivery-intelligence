import { useId, useMemo, type CSSProperties } from 'react'

import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import { buildJobDag, type JobPlacement } from '../../lib/job-dag'
import type { PipelineJob } from '../../lib/types'
import { count } from '@/lib/intl'

/* Fixed geometry: positions derive from (level, row), so no measuring is needed. */
const CHIP_WIDTH = 120
const CHIP_HEIGHT = 24
const GAP_X = 28
const GAP_Y = 8
const PITCH_X = CHIP_WIDTH + GAP_X
const PITCH_Y = CHIP_HEIGHT + GAP_Y

function linkPath(from: JobPlacement, to: JobPlacement) {
  const x1 = from.level * PITCH_X + CHIP_WIDTH
  const y1 = from.row * PITCH_Y + CHIP_HEIGHT / 2
  const x2 = to.level * PITCH_X
  const y2 = to.row * PITCH_Y + CHIP_HEIGHT / 2
  const bend = Math.max(12, (x2 - x1) / 2)
  return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`
}

interface JobChipProps {
  job: PipelineJob
  style: CSSProperties
}

function JobChip({ job, style }: JobChipProps) {
  const steps = job.steps.length
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          tabIndex={0}
          className="absolute flex items-center rounded-md bg-card px-2 text-2xs ring-1 ring-border focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none"
          style={style}
        >
          <span className="truncate">{job.name || job.key}</span>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom" className="flex flex-col items-start gap-0.5">
        <span className="font-mono">{job.key}</span>
        <span>
          {job.runsOn ? `Runs on ${job.runsOn}` : 'Runner not set'} · {count(steps, 'step', 'steps')}
        </span>
      </TooltipContent>
    </Tooltip>
  )
}

interface JobDagProps {
  jobs: PipelineJob[]
}

/** Jobs as chips in columns by dependency depth, with a curve per `needs` entry. */
export function JobDag({ jobs }: JobDagProps) {
  const dag = useMemo(() => buildJobDag(jobs), [jobs])
  const markerId = `job-arrow-${useId().replaceAll(/[^a-zA-Z0-9]/g, '')}`

  if (jobs.length === 0) {
    return <p className="text-2xs text-muted-foreground">No jobs parsed from this definition.</p>
  }

  const levels = dag.columns.length
  const rows = Math.max(...dag.columns.map((column) => column.length))
  const width = levels * PITCH_X - GAP_X
  const height = rows * PITCH_Y - GAP_Y

  return (
    <div className="overflow-x-auto pb-1 [scrollbar-gutter:stable]" role="group" aria-label={`${jobs.length} jobs`}>
      <div className="relative" style={{ width, height }}>
        <svg className="absolute inset-0 overflow-visible text-muted-foreground/60" width={width} height={height} aria-hidden>
          <defs>
            <marker
              id={markerId}
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="7"
              markerHeight="7"
              markerUnits="userSpaceOnUse"
              orient="auto"
            >
              <path d="M 2 2 L 8 5 L 2 8" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
            </marker>
          </defs>
          {dag.links.map((link) => {
            const from = dag.placements.get(link.from)
            const to = dag.placements.get(link.to)
            if (!from || !to) return null
            return (
              <path
                key={`${link.from}->${link.to}`}
                d={linkPath(from, to)}
                fill="none"
                stroke="currentColor"
                strokeWidth={1}
                markerEnd={`url(#${markerId})`}
              />
            )
          })}
        </svg>
        {dag.columns.map((column, level) =>
          column.map((job, row) => (
            <JobChip
              key={job.key}
              job={job}
              style={{ left: level * PITCH_X, top: row * PITCH_Y, width: CHIP_WIDTH, height: CHIP_HEIGHT }}
            />
          )),
        )}
      </div>
    </div>
  )
}
