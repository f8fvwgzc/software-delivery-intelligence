import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { QUEUE_KIND, QUEUE_KINDS } from '../../lib/labels'
import type { QueueFilter } from '../../lib/work-queue'

interface QueueFilterProps {
  value: QueueFilter
  counts: Record<QueueFilter, number>
  onChange: (value: QueueFilter) => void
}

/** Narrow the queue to one kind. Counts stay visible so nothing is hidden silently. */
export function QueueFilterGroup({ value, counts, onChange }: QueueFilterProps) {
  return (
    <ToggleGroup
      type="single"
      size="sm"
      spacing={1}
      value={value}
      onValueChange={(next) => {
        // Radix reports '' when the active item is clicked again; keep the current filter.
        if (next) onChange(next as QueueFilter)
      }}
      aria-label="Filter the work queue by kind"
    >
      <ToggleGroupItem value="all" className="gap-1.5">
        All
        <span className="tabular-nums text-muted-foreground">{counts.all}</span>
      </ToggleGroupItem>
      {QUEUE_KINDS.map((kind) => (
        <ToggleGroupItem key={kind} value={kind} className="gap-1.5">
          {QUEUE_KIND[kind].label}
          <span className="tabular-nums text-muted-foreground">{counts[kind]}</span>
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  )
}
