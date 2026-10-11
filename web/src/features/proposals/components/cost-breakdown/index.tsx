import { Table, TableBody, TableCell, TableFooter, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { formatUsd } from '../../lib/format'

type CostLine = components['schemas']['CostLine']

interface CostBreakdownProps {
  lines: CostLine[]
  /** The option's `estimatedMonthlyCostUsd`; the lines are expected to sum to it. */
  estimateMonthlyUsd: number
  /** Tighter rows and no notes column, for a popover. */
  compact?: boolean
  className?: string
}

const CENT = 0.005

/** The cost model behind an estimate: one tabular row per billed item, total pinned to the estimate. */
export function CostBreakdown({ lines, estimateMonthlyUsd, compact = false, className }: CostBreakdownProps) {
  if (lines.length === 0) {
    return (
      <p className={cn('text-xs text-muted-foreground', className)}>
        {estimateMonthlyUsd <= 0 ? 'Nothing is billed for this option.' : 'No cost model was recorded for this option.'}
      </p>
    )
  }

  const sum = lines.reduce((total, line) => total + line.monthlyUsd, 0)
  const mismatch = Math.abs(sum - estimateMonthlyUsd) > CENT
  const cell = compact ? 'px-2 py-1' : 'px-3 py-1.5'

  return (
    <div className={cn('flex flex-col gap-1.5', className)}>
      <div className="overflow-x-auto rounded-md ring-1 ring-border/60">
        <Table className="text-xs">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className={cn('h-8 text-2xs font-medium text-muted-foreground', cell)}>Item</TableHead>
              {!compact ? (
                <TableHead className={cn('h-8 text-2xs font-medium text-muted-foreground', cell)}>How it was estimated</TableHead>
              ) : null}
              <TableHead className={cn('h-8 w-24 text-right text-2xs font-medium text-muted-foreground', cell)}>Per month</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {lines.map((line, index) => (
              <TableRow key={`${line.item}-${index}`} className="hover:bg-transparent">
                <TableCell className={cn('font-mono', cell)}>{line.item}</TableCell>
                {!compact ? <TableCell className={cn('max-w-0 text-muted-foreground', cell)}>{line.note}</TableCell> : null}
                <TableCell className={cn('text-right tabular-nums', cell)}>{formatUsd(line.monthlyUsd)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
          <TableFooter className="bg-muted/30">
            <TableRow className="hover:bg-transparent">
              <TableCell className={cn('font-medium', cell)} colSpan={compact ? 1 : 2}>
                Estimate
              </TableCell>
              <TableCell className={cn('text-right font-medium tabular-nums', cell)}>{formatUsd(estimateMonthlyUsd)}</TableCell>
            </TableRow>
          </TableFooter>
        </Table>
      </div>
      {mismatch ? (
        <p className="text-2xs text-warning">
          The lines add up to {formatUsd(sum)}, not the {formatUsd(estimateMonthlyUsd)} estimate.
        </p>
      ) : null}
    </div>
  )
}
