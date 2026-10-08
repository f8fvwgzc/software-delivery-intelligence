import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { NODE_KIND_META } from '../../lib/labels'
import type { NodeKind } from '../../lib/types'

interface KindFilterProps {
  /** Kinds present in the graph, in display order. */
  kinds: readonly NodeKind[]
  selected: readonly NodeKind[]
  onChange: (kinds: NodeKind[]) => void
}

/** Multi-select chips, one per kind present in the graph. */
export function KindFilter({ kinds, selected, onChange }: KindFilterProps) {
  return (
    <ToggleGroup
      type="multiple"
      value={selected as string[]}
      onValueChange={(value) => onChange(value as NodeKind[])}
      size="sm"
      variant="outline"
      spacing={1}
      aria-label="Filter by node kind"
      className="flex-wrap"
    >
      {kinds.map((kind) => {
        const meta = NODE_KIND_META[kind]
        const Icon = meta.icon
        return (
          <ToggleGroupItem key={kind} value={kind} className="h-7 gap-1.5 px-2 text-xs font-normal data-[state=on]:text-foreground">
            <Icon data-icon="inline-start" className="size-3.5 text-muted-foreground" />
            {meta.label}
          </ToggleGroupItem>
        )
      })}
    </ToggleGroup>
  )
}
