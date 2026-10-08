import { useState } from 'react'

import { Kbd, KbdGroup } from '@/components/ui/kbd'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { PipelineRun } from '../../lib/types'
import { RunRow } from './run-row'

interface RunsTableProps {
  runs: PipelineRun[]
}

/** Recent runs, newest first. Failed runs expand in place to show the cause. */
export function RunsTable({ runs }: RunsTableProps) {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())

  const toggle = (id: string) =>
    setExpanded((previous) => {
      const next = new Set(previous)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  return (
    <div className="flex flex-col gap-2">
      {/* `@container` lets the expanded failure row size itself to the visible width when the table scrolls sideways. */}
      <div className="@container overflow-hidden rounded-lg ring-1 ring-border">
        <Table className="[&_td]:px-3 [&_td]:py-2 [&_th]:px-3 [&_th]:text-xs [&_th]:font-medium [&_th]:text-muted-foreground">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="w-8">
                <span className="sr-only">Details</span>
              </TableHead>
              <TableHead>Status</TableHead>
              <TableHead>Run</TableHead>
              <TableHead>Workflow</TableHead>
              <TableHead>Branch</TableHead>
              <TableHead>Commit</TableHead>
              <TableHead>Actor</TableHead>
              <TableHead>Started</TableHead>
              <TableHead className="text-right">Duration</TableHead>
              <TableHead className="w-8">
                <span className="sr-only">Open at the provider</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {runs.map((run) => (
              <RunRow key={run.id} run={run} expanded={expanded.has(run.id)} onToggle={toggle} />
            ))}
          </TableBody>
        </Table>
      </div>
      <p className="hidden items-center justify-end gap-1.5 text-2xs text-muted-foreground md:flex">
        <KbdGroup>
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
        </KbdGroup>
        move between runs
        <span aria-hidden>·</span>
        <Kbd>Enter</Kbd>
        expand a failed run
      </p>
    </div>
  )
}

export { RunsSkeleton } from './skeleton'
