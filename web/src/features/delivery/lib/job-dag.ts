import type { PipelineJob } from './types'

export interface JobPlacement {
  /** Column: 0 for jobs with no needs, 1 + the deepest need otherwise. */
  level: number
  /** Row inside the column, in definition order. */
  row: number
}

export interface JobLink {
  from: string
  to: string
}

export interface JobDag {
  columns: PipelineJob[][]
  placements: Map<string, JobPlacement>
  links: JobLink[]
}

/** Layers jobs by their `needs` so they can be drawn left to right. Cycles and unknown needs fall back to level 0. */
export function buildJobDag(jobs: PipelineJob[]): JobDag {
  const byKey = new Map(jobs.map((job) => [job.key, job]))
  const levels = new Map<string, number>()
  const visiting = new Set<string>()

  function levelOf(key: string): number {
    const known = levels.get(key)
    if (known !== undefined) return known
    const job = byKey.get(key)
    if (!job || visiting.has(key)) return 0

    visiting.add(key)
    let level = 0
    for (const need of job.needs) {
      if (byKey.has(need) && need !== key) level = Math.max(level, levelOf(need) + 1)
    }
    visiting.delete(key)
    levels.set(key, level)
    return level
  }

  const columns: PipelineJob[][] = []
  const placements = new Map<string, JobPlacement>()
  for (const job of jobs) {
    if (placements.has(job.key)) continue
    const level = levelOf(job.key)
    while (columns.length <= level) columns.push([])
    const column = columns[level]
    placements.set(job.key, { level, row: column.length })
    column.push(job)
  }

  const seen = new Set<string>()
  const links: JobLink[] = []
  for (const job of jobs) {
    for (const need of job.needs) {
      const id = `${need}->${job.key}`
      if (!byKey.has(need) || need === job.key || seen.has(id)) continue
      seen.add(id)
      links.push({ from: need, to: job.key })
    }
  }

  return { columns, placements, links }
}
