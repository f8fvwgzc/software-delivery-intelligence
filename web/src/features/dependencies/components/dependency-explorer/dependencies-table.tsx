import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { cn } from '@/lib/utils'

import type { Dependency } from '../../api/queries'
import { ecosystemLabel, scopeLabel } from '../../lib/labels'
import { CELL_CLASS, HEAD_CLASS } from './columns'

interface DependenciesTableProps {
  rows: Dependency[]
  /** Dims the rows while a deferred search catches up with the input. */
  stale?: boolean
}

export function DependenciesTable({ rows, stale = false }: DependenciesTableProps) {
  return (
    <div className={cn('overflow-hidden rounded-lg ring-1 ring-border transition-opacity', stale && 'opacity-60')}>
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className={HEAD_CLASS}>Package</TableHead>
            <TableHead className={HEAD_CLASS}>Ecosystem</TableHead>
            <TableHead className={HEAD_CLASS}>Version</TableHead>
            <TableHead className={HEAD_CLASS}>Scope</TableHead>
            <TableHead className={HEAD_CLASS}>Manifest</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((dependency) => (
            <TableRow key={dependency.id}>
              <TableCell className={CELL_CLASS.name}>{dependency.name}</TableCell>
              <TableCell className={CELL_CLASS.ecosystem}>{ecosystemLabel(dependency.ecosystem)}</TableCell>
              <TableCell className={CELL_CLASS.version}>{dependency.version || '—'}</TableCell>
              <TableCell className={cn(CELL_CLASS.scope, dependency.scope !== 'runtime' && 'text-muted-foreground')}>
                {scopeLabel(dependency.scope)}
              </TableCell>
              <TableCell className={CELL_CLASS.manifest}>
                <span className="block max-w-72 truncate" title={dependency.manifestPath}>
                  {dependency.manifestPath}
                </span>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
