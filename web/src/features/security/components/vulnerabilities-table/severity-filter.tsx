import { Tag } from '@/components/system/tag'
import { cn } from '@/lib/utils'

import type { SeverityCount } from '../../api/types'
import { severityMeta } from '../../lib/labels'

/** Sentinel for "every severity"; no severity the server sends is spelled like it. */
export const ALL_SEVERITIES = 'all'

interface SeverityFilterProps {
  /** Counts per severity as the server sent them, most severe first. */
  counts: readonly SeverityCount[]
  total: number
  value: string
  onChange: (value: string) => void
}

const BUTTON_CLASS = 'inline-flex rounded-[4px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50'

/**
 * The severities that have something open, as tags that filter the table:
 * one at a time, "All" to clear. The chosen one is tinted, the rest are
 * outlined.
 */
export function SeverityFilter({ counts, total, value, onChange }: SeverityFilterProps) {
  const present = counts.filter((row) => row.count > 0)
  // One severity only: there is nothing to narrow down.
  if (present.length < 2) return null

  return (
    <div role="group" aria-label="Filter by severity" className="flex flex-wrap items-center gap-1.5">
      <button type="button" aria-pressed={value === ALL_SEVERITIES} onClick={() => onChange(ALL_SEVERITIES)} className={BUTTON_CLASS}>
        <Tag tone="neutral" variant={value === ALL_SEVERITIES ? 'tint' : 'outline'} className={cn(value === ALL_SEVERITIES && 'text-foreground')}>
          All <span className="tabular-nums">{total.toLocaleString()}</span>
        </Tag>
      </button>
      {present.map((row) => {
        const meta = severityMeta(row.severity)
        const selected = value === row.severity
        return (
          <button
            key={row.severity}
            type="button"
            aria-pressed={selected}
            // Choosing the selected one again clears it.
            onClick={() => onChange(selected ? ALL_SEVERITIES : row.severity)}
            className={BUTTON_CLASS}
          >
            <Tag tone={meta.tone} variant={selected ? 'tint' : 'outline'}>
              {meta.label} <span className="tabular-nums">{row.count.toLocaleString()}</span>
            </Tag>
          </button>
        )
      })}
    </div>
  )
}
