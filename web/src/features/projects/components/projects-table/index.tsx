import { useRef } from 'react'

import { useWindowRows } from '@/components/system/virtual-rows'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { Project } from '../../api/queries'
import { HEAD_CLASS, ROW_HEIGHT } from './columns'
import { ProjectRow } from './project-row'

interface ProjectsTableProps {
  projects: Project[]
}

/**
 * Dense list of connected repositories. One row per project, actions
 * inline. Past 200 projects only the rows on screen are in the DOM (the
 * table scrolls with the page; spacer rows keep its height and scrollbar).
 */
export function ProjectsTable({ projects }: ProjectsTableProps) {
  const bodyRef = useRef<HTMLTableSectionElement>(null)
  const rows = useWindowRows({ count: projects.length, estimateSize: ROW_HEIGHT, listRef: bodyRef })

  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table data-virtual={rows.virtual || undefined}>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className={HEAD_CLASS}>Repository</TableHead>
            <TableHead className={HEAD_CLASS}>Status</TableHead>
            <TableHead className={HEAD_CLASS}>Mode</TableHead>
            <TableHead className={HEAD_CLASS}>Stack</TableHead>
            <TableHead className={HEAD_CLASS}>Maturity</TableHead>
            <TableHead className={`${HEAD_CLASS} text-right`}>Open questions</TableHead>
            <TableHead className={HEAD_CLASS}>Last scanned</TableHead>
            <TableHead className={HEAD_CLASS}>
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody ref={bodyRef}>
          {rows.virtual && rows.offsetTop > 0 ? <SpacerRow height={rows.offsetTop} /> : null}
          {rows.items.map((item) => {
            const project = projects[item.index]
            return project ? <ProjectRow key={project.id} project={project} index={item.index} measure={rows.measure} /> : null
          })}
          {rows.virtual && rows.offsetBottom > 0 ? <SpacerRow height={rows.offsetBottom} /> : null}
        </TableBody>
      </Table>
    </div>
  )
}

/** Keeps the table as tall as every row while only a window of them is rendered. */
function SpacerRow({ height }: { height: number }) {
  return (
    <TableRow aria-hidden className="border-0 hover:bg-transparent" style={{ height }}>
      <td colSpan={8} className="p-0" />
    </TableRow>
  )
}

export { ProjectsTableSkeleton } from './projects-table-skeleton'
