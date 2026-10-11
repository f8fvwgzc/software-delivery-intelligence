import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { Project } from '../../api/queries'
import { HEAD_CLASS } from './columns'
import { ProjectRow } from './project-row'

interface ProjectsTableProps {
  projects: Project[]
}

/** Dense list of connected repositories. One row per project, actions inline. */
export function ProjectsTable({ projects }: ProjectsTableProps) {
  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
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
        <TableBody>
          {projects.map((project) => (
            <ProjectRow key={project.id} project={project} />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

export { ProjectsTableSkeleton } from './projects-table-skeleton'
