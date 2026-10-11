import { CheckIcon } from 'lucide-react'

import { ScoreBar } from '@/components/shared/data'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverDescription, PopoverHeader, PopoverTitle, PopoverTrigger } from '@/components/ui/popover'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { isOverBudget, overBudgetBy } from '../../lib/budget'
import { formatMonthlyCost, formatPercent, formatUsd } from '../../lib/format'
import { CostBreakdown } from '../cost-breakdown'

type ProposalOption = components['schemas']['ProposalOption']

interface OptionsTableProps {
  options: ProposalOption[]
  recommendedId: string
  selectedId: string
  onChoose: (optionId: string) => void
  /** Read-only once the proposal is closed. */
  readOnly?: boolean
  /** Monthly budget the options are marked against, if any. */
  budgetMonthlyUsd?: number | null
}

/** Compact comparison of every option, best rank first. The cost cell opens the option's cost model. */
export function OptionsTable({
  options,
  recommendedId,
  selectedId,
  onChoose,
  readOnly = false,
  budgetMonthlyUsd = null,
}: OptionsTableProps) {
  const ranked = options.toSorted((a, b) => a.rank - b.rank)

  return (
    <div className="overflow-x-auto rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className="w-10 px-3 text-right text-xs font-medium text-muted-foreground">#</TableHead>
            <TableHead className="px-3 text-xs font-medium text-muted-foreground">Option</TableHead>
            <TableHead className="w-36 px-3 text-xs font-medium text-muted-foreground">Overall</TableHead>
            <TableHead className="w-28 px-3 text-right text-xs font-medium text-muted-foreground">Cost</TableHead>
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
            const overBudget = isOverBudget(option, budgetMonthlyUsd)
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
                <TableCell className="px-1.5 text-right">
                  <CostCell option={option} overBudget={overBudget} budgetMonthlyUsd={budgetMonthlyUsd} />
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

interface CostCellProps {
  option: ProposalOption
  overBudget: boolean
  budgetMonthlyUsd: number | null
}

/** The estimate as a quiet trigger; the popover holds the lines behind it. */
function CostCell({ option, overBudget, budgetMonthlyUsd }: CostCellProps) {
  const excess = overBudgetBy(option, budgetMonthlyUsd)
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="xs"
          className={cn(
            'h-7 flex-col items-end gap-0 px-1.5 font-normal tabular-nums',
            overBudget ? 'text-warning hover:text-warning' : 'text-foreground',
          )}
          aria-label={`Cost model for ${option.title}`}
        >
          <span>{formatMonthlyCost(option.estimatedMonthlyCostUsd)}</span>
          {overBudget ? (
            <span className="text-2xs leading-3">{excess > 0 ? `${formatUsd(excess)} over` : 'over budget'}</span>
          ) : null}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80">
        <PopoverHeader>
          <PopoverTitle className="truncate">{option.title}</PopoverTitle>
          <PopoverDescription className="text-xs">
            {budgetMonthlyUsd !== null
              ? overBudget
                ? `Budget is ${formatMonthlyCost(budgetMonthlyUsd)}; this option exceeds it.`
                : `Within the ${formatMonthlyCost(budgetMonthlyUsd)} budget.`
              : 'Estimated monthly cost by item.'}
          </PopoverDescription>
        </PopoverHeader>
        <CostBreakdown lines={option.costBreakdown} estimateMonthlyUsd={option.estimatedMonthlyCostUsd} compact />
      </PopoverContent>
    </Popover>
  )
}
