import { ChevronRightIcon } from 'lucide-react'
import { useState } from 'react'

import { ScoreBar } from '@/components/shared/data'
import { TechChips } from '@/components/shared/data/tech-chip'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { TableCell, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'
import { cn } from '@/lib/utils'

import { categoryLabel, cloudLabel, humanize, PATTERN_SOURCE } from '../../lib/labels'
import { ComplexityDots } from './complexity-dots'
import { PatternDetails } from './pattern-details'

type Pattern = components['schemas']['Pattern']

const usd = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 })

const DOT_CLASS = {
  neutral: 'bg-muted-foreground/60',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  destructive: 'bg-destructive',
  brand: 'bg-brand',
} as const

/**
 * One pattern is its own <tbody>, so the summary row and the expanded
 * details row stay valid table markup under one Collapsible.
 */
export function PatternRow({ pattern }: { pattern: Pattern }) {
  const [open, setOpen] = useState(false)
  const source = PATTERN_SOURCE[pattern.source]
  const deprecated = pattern.status === 'deprecated'

  return (
    <Collapsible asChild open={open} onOpenChange={setOpen}>
      <tbody className={cn(deprecated && 'text-muted-foreground')}>
        <TableRow className="h-11" data-state={open ? 'open' : 'closed'}>
          <TableCell className="px-2">
            <CollapsibleTrigger asChild>
              <button
                type="button"
                aria-label={open ? `Collapse ${pattern.name}` : `Expand ${pattern.name}`}
                className="flex size-6 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
              >
                <ChevronRightIcon
                  className={cn('size-4 transition-transform duration-150 ease-out', open && 'rotate-90')}
                />
              </button>
            </CollapsibleTrigger>
          </TableCell>
          <TableCell className="max-w-0 px-3">
            <div className="flex min-w-0 items-center gap-2">
              <span className={cn('size-1.5 shrink-0 rounded-full', DOT_CLASS[source.tone])} title={source.label} />
              <span className="truncate font-medium">{pattern.name}</span>
              {deprecated ? <span className="shrink-0 text-2xs">deprecated</span> : null}
            </div>
          </TableCell>
          <TableCell className="px-3 text-xs">{categoryLabel(pattern.category)}</TableCell>
          <TableCell className="px-3">
            <TechChips items={pattern.technologies} max={3} />
          </TableCell>
          <TableCell className="px-3 text-xs">
            <div className="flex flex-col leading-4">
              <span>{cloudLabel(pattern.cloud)}</span>
              <span className="text-2xs text-muted-foreground">
                {humanize(pattern.runtime)} · {humanize(pattern.iac)}
              </span>
            </div>
          </TableCell>
          <TableCell className="px-3">
            <div className="flex flex-col gap-0.5">
              <ScoreBar value={pattern.successRate} />
              <span className="text-2xs tabular-nums text-muted-foreground">
                {pattern.deployments} {pattern.deployments === 1 ? 'deployment' : 'deployments'}
                {pattern.incidents > 0 ? `, ${pattern.incidents} incidents` : ''}
              </span>
            </div>
          </TableCell>
          <TableCell className="px-3 text-right text-xs tabular-nums">
            {pattern.estimatedMonthlyCostUsd <= 0 ? 'Free' : `${usd.format(pattern.estimatedMonthlyCostUsd)}/mo`}
          </TableCell>
          <TableCell className="px-3">
            <ComplexityDots value={pattern.complexity} />
          </TableCell>
          <TableCell className="max-w-0 px-3 text-xs">
            {pattern.usedBy.length === 0 ? (
              <span className="text-muted-foreground">Not used yet</span>
            ) : (
              <span className="block truncate" title={pattern.usedBy.join(', ')}>
                {pattern.usedBy.join(', ')}
              </span>
            )}
          </TableCell>
        </TableRow>
        <CollapsibleContent asChild>
          <tr className="border-b bg-muted/30">
            <td />
            <td colSpan={8} className="px-3 py-4">
              <PatternDetails pattern={pattern} />
            </td>
          </tr>
        </CollapsibleContent>
      </tbody>
    </Collapsible>
  )
}
