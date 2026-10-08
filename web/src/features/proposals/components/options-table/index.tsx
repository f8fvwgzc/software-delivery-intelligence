import { CheckIcon } from 'lucide-react'

import { ScoreBar } from '@/components/shared/data'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { formatMonthlyCost, formatPercent } from '../../lib/format'

type ProposalOption = components['schemas']['ProposalOption']

interface OptionsTableProps {
  options: ProposalOption[]
  recommendedId: string
  selectedId: string
  onChoose: (optionId: string) => void
  /** Read-only once the proposal is closed. */
  readOnly?: boolean
}

/** Compact comparison of every option, best rank first. */
export function OptionsTable({ options, recommendedId, selectedId, onChoose, readOnly = false }: OptionsTableProps) {
  const ranked = options.toSorted((a, b) => a.rank - b.rank)

  return (
    <div className="overflow-x-auto rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-10 px-3 text-right text-xs font-medium text-muted-foreground">#</TableHead>
            <TableHead className="px-3 text-xs font-medium text-muted-foreground">Option</TableHead>
            <TableHead className="w-36 px-3 text-xs font-medium text-muted-foreground">Overall</TableHead>
            <TableHead className="w-24 px-3 text-right text-xs font-medium text-muted-foreground">Cost</TableHead>
            <TableHead className="w-20 px-3 text-right text-xs font-medium text-muted-foreground">Security</TableHead>
            <TableHead className="w-24 px-3 text-right text-xs font-medium text-muted-foreground">Fit</TableHead>
            <TableHead className="w-24 px-3 text-right text-xs font-medium text-muted-foreground">
              <span className="sr-only">Choose</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {ranked.map((option) => {
            const selected = option.id === selectedId
            const recommended = option.id === recommendedId
            return (
              <TableRow
                key={option.id}
                data-state={selected ? 'selected' : undefined}
                className={cn('h-11', selected && 'shadow-[inset_2px_0_0_0_var(--brand)]')}
              >
                <TableCell className="px-3 text-right font-mono text-xs tabular-nums text-muted-foreground">
                  {option.rank}
                </TableCell>
                <TableCell className="max-w-0 px-3">
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="truncate font-medium">{option.title}</span>
                    {recommended ? (
                      <span className="shrink-0 text-2xs text-muted-foreground">recommended</span>
                    ) : null}
                  </div>
                </TableCell>
                <TableCell className="px-3">
                  <ScoreBar value={option.score?.overall ?? 0} />
                </TableCell>
                <TableCell className="px-3 text-right text-xs tabular-nums">
                  {formatMonthlyCost(option.estimatedMonthlyCostUsd)}
                </TableCell>
                <TableCell className="px-3 text-right text-xs tabular-nums">
                  {formatPercent(option.score?.security ?? 0)}
                </TableCell>
                <TableCell className="px-3 text-right text-xs tabular-nums">
                  {formatPercent(option.score?.architectureFit ?? 0)}
                </TableCell>
                <TableCell className="px-3 text-right">
                  {selected ? (
                    <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                      <CheckIcon className="size-3.5" />
                      In focus
                    </span>
                  ) : (
                    <Button variant="ghost" size="xs" disabled={readOnly} onClick={() => onChoose(option.id)}>
                      Choose
                    </Button>
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
