import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { RUNTIME_ADAPTERS, adapterLabel, isRuntimeAdapter, type Adapter } from '../../lib/labels'

export type AdapterFilter = 'all' | Adapter

interface AdapterFilterProps {
  value: AdapterFilter
  onChange: (value: AdapterFilter) => void
  /** Executions per adapter, for the counts on each item. */
  counts: Partial<Record<AdapterFilter, number>>
}

/** all / dry run / docker / github. A Radix toggle group can deselect; "all" is the floor. */
export function AdapterFilterGroup({ value, onChange, counts }: AdapterFilterProps) {
  return (
    <ToggleGroup
      type="single"
      size="sm"
      variant="outline"
      spacing={0}
      value={value}
      onValueChange={(next) => onChange(next === 'all' || isRuntimeAdapter(next) ? next : 'all')}
      aria-label="Filter by adapter"
    >
      <ToggleGroupItem value="all" className="text-xs">
        All
        <Count value={counts.all} />
      </ToggleGroupItem>
      {RUNTIME_ADAPTERS.map((adapter) => (
        <ToggleGroupItem key={adapter} value={adapter} className="text-xs">
          {adapterLabel(adapter)}
          <Count value={counts[adapter]} />
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
}

function Count({ value }: { value: number | undefined }) {
  if (value === undefined) return null
  return <span className="ml-1 tabular-nums text-muted-foreground">{value}</span>
}
