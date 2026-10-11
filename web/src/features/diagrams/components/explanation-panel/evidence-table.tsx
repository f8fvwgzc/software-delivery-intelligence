import { ArrowRightIcon } from 'lucide-react'
import { Link } from 'react-router'

import { EmptyState } from '@/components/shared/feedback'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { DiagramEdge, DiagramNode } from '../../api/queries'
import { codeBrowserPath } from '../../lib/code-path'
import { EDGE_STYLE, edgeStyle } from '../../lib/labels'

interface EvidenceTableProps {
  projectId: string
  edges: readonly DiagramEdge[]
  nodes: readonly DiagramNode[]
}

/** Compact: source → target, label, style, and the evidence file linking to the code browser. */
export function EvidenceTable({ projectId, edges, nodes }: EvidenceTableProps) {
  if (edges.length === 0) {
    return (
      <EmptyState
        title="No edges"
        description="This diagram has nodes only, so there is nothing to cite."
        className="py-6"
      />
    )
  }

  const labelOf = new Map(nodes.map((node) => [node.id, node.label]))

  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader className="bg-muted/40">
          <TableRow className="hover:bg-transparent">
            <TableHead className="h-9 text-xs">Relationship</TableHead>
            <TableHead className="h-9 text-xs">Label</TableHead>
            <TableHead className="h-9 text-xs">Evidence</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {edges.map((edge, index) => {
            const evidence = edge.evidencePath?.trim() ?? ''
            return (
              <TableRow key={edge.id || `${edge.source}-${edge.target}-${index}`}>
                <TableCell className="max-w-64">
                  <span className="flex min-w-0 items-center gap-1.5 text-xs">
                    <span className="truncate">{labelOf.get(edge.source) ?? edge.source}</span>
                    <ArrowRightIcon className="size-3 shrink-0 text-muted-foreground" aria-label="to" />
                    <span className="truncate">{labelOf.get(edge.target) ?? edge.target}</span>
                  </span>
                </TableCell>
                <TableCell className="text-xs text-muted-foreground">
                  {edge.label?.trim() ? edge.label : EDGE_STYLE[edgeStyle(edge.style)].label.toLowerCase()}
                </TableCell>
                <TableCell className="max-w-72">
                  {evidence ? (
                    <Link
                      to={codeBrowserPath(projectId, evidence)}
                      className="block truncate rounded-sm font-mono text-xs outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
                      title={evidence}
                    >
                      {evidence}
                    </Link>
                  ) : (
                    <span className="text-xs text-muted-foreground italic">inferred</span>
                  )}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
