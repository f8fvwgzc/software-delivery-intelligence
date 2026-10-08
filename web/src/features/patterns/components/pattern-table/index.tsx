import { LayersIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Table, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'

import { PatternRow } from './pattern-row'

type Pattern = components['schemas']['Pattern']

interface PatternTableProps {
  patterns: Pattern[]
  /** True when a filter is active, so the empty state can say so. */
  filtered: boolean
}

/** Catalog and learned patterns. Each row expands in place. */
export function PatternTable({ patterns, filtered }: PatternTableProps) {
  if (patterns.length === 0) {
    return (
      <EmptyState
        icon={LayersIcon}
        title={filtered ? 'No patterns match' : 'No patterns yet'}
        description={
          filtered
            ? 'Try another category, or include deprecated patterns.'
            : 'The catalog is seeded by the knowledge service on first start.'
        }
      />
    )
  }

  return (
    <div className="overflow-x-auto rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-8 px-2">
              <span className="sr-only">Expand</span>
            </TableHead>
            <TableHead className="px-3 text-xs font-medium text-muted-foreground">Pattern</TableHead>
            <TableHead className="w-28 px-3 text-xs font-medium text-muted-foreground">Category</TableHead>
            <TableHead className="w-48 px-3 text-xs font-medium text-muted-foreground">Technologies</TableHead>
            <TableHead className="w-36 px-3 text-xs font-medium text-muted-foreground">Cloud · runtime · IaC</TableHead>
            <TableHead className="w-40 px-3 text-xs font-medium text-muted-foreground">Success</TableHead>
            <TableHead className="w-20 px-3 text-right text-xs font-medium text-muted-foreground">Cost</TableHead>
            <TableHead className="w-20 px-3 text-xs font-medium text-muted-foreground">Complexity</TableHead>
            <TableHead className="w-32 px-3 text-xs font-medium text-muted-foreground">Used by</TableHead>
          </TableRow>
        </TableHeader>
        {patterns.map((pattern) => (
          <PatternRow key={pattern.id} pattern={pattern} />
        ))}
      </Table>
    </div>
  )
}
