import { cn } from '@/lib/utils'

import type { EcosystemCount } from '../../api/queries'
import { ALL_ECOSYSTEMS, ecosystemLabel } from '../../lib/labels'

interface EcosystemSummaryProps {
  counts: EcosystemCount[]
  total: number
  /** Currently selected ecosystem, or `ALL_ECOSYSTEMS`. */
  selected: string
  onSelect: (ecosystem: string) => void
}

/**
 * Horizontal bars, one hue, largest first. Each row is a filter: clicking
 * an ecosystem scopes the table to it, clicking it again clears the filter.
 */
export function EcosystemSummary({ counts, total, selected, onSelect }: EcosystemSummaryProps) {
  const sorted = counts.toSorted((a, b) => b.count - a.count)
  const max = sorted[0]?.count ?? 0

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm">
        <span className="text-lg font-semibold tabular-nums">{total.toLocaleString()}</span>{' '}
        <span className="text-muted-foreground">
          {total === 1 ? 'dependency' : 'dependencies'} across {sorted.length}{' '}
          {sorted.length === 1 ? 'ecosystem' : 'ecosystems'}
        </span>
      </p>
      <ul className="flex flex-col gap-1.5">
        {sorted.map((row, index) => {
          const isSelected = selected === row.ecosystem
          const width = max > 0 ? Math.max(2, Math.round((row.count / max) * 100)) : 0

          return (
            <li key={row.ecosystem}>
              <button
                type="button"
                aria-pressed={isSelected}
                onClick={() => onSelect(isSelected ? ALL_ECOSYSTEMS : row.ecosystem)}
                className={cn(
                  'grid w-full grid-cols-[7rem_minmax(0,1fr)_3rem] items-center gap-3 rounded-md px-1.5 py-1 text-left text-xs outline-none transition-colors hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50',
                  isSelected && 'bg-muted',
                )}
              >
                <span className={cn('truncate', isSelected ? 'font-medium' : 'text-muted-foreground')}>
                  {ecosystemLabel(row.ecosystem)}
                </span>
                <span className="h-2 overflow-hidden rounded-sm bg-muted">
                  <span
                    className={cn(
                      'block h-full rounded-sm transition-[width] duration-300 ease-out',
                      index === 0 || isSelected ? 'bg-brand' : 'bg-brand/55',
                    )}
                    style={{ width: `${width}%` }}
                  />
                </span>
                <span className="text-right tabular-nums">{row.count.toLocaleString()}</span>
              </button>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
