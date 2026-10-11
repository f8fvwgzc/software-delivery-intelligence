import { ShapesIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { EmptyState } from '@/components/shared/feedback'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { Diagram } from '../../api/queries'
import { DiagramRow } from './row'

export { DiagramsTableSkeleton } from './skeleton'

interface DiagramsTableProps {
  diagrams: readonly Diagram[]
  /** Rendered under the empty state's text, e.g. a hint pointing at the generate form. */
  emptyAction?: ReactNode
}

/** Every diagram of the project, newest first, as a dense table. */
export function DiagramsTable({ diagrams, emptyAction }: DiagramsTableProps) {
  if (diagrams.length === 0) {
    return (
      <EmptyState
        icon={ShapesIcon}
        title="No diagrams yet"
        description="A diagram is a set of groups, nodes and edges built from the knowledge graph, the repository tree and a few key files. Every node links to the file that implements it and every edge names its evidence. Pick a kind above to generate the first one."
        action={emptyAction}
      />
    )
  }

  const sorted = diagrams.toSorted((a, b) => b.updatedAt.localeCompare(a.updatedAt))

  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader className="bg-muted/40">
          <TableRow className="hover:bg-transparent">
            <TableHead className="h-10 text-xs">Title</TableHead>
            <TableHead className="h-10 text-xs">Kind</TableHead>
            <TableHead className="h-10 text-xs">Source</TableHead>
            <TableHead className="h-10 text-xs">Revision</TableHead>
            <TableHead className="h-10 text-xs">Commit</TableHead>
            <TableHead className="h-10 text-right text-xs">Updated</TableHead>
            <TableHead className="h-10 w-10">
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((diagram) => (
            <DiagramRow key={diagram.id} diagram={diagram} />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
