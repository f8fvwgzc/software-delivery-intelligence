import { Maximize2Icon } from 'lucide-react'
import { useMemo, useState } from 'react'

import { Row, RowList } from '@/components/system'
import { Button } from '@/components/ui/button'

import { connections, descendantCount, type Connection, type Hierarchy } from '../../lib/board'
import { NODE_KIND_META, relationLabel } from '../../lib/labels'

interface NodeConnectionsProps {
  hierarchy: Hierarchy
  nodeId: string
  /** Open the node's own board (shown when something is folded inside it). */
  onOpen: (nodeId: string) => void
  /** Show a neighbour in its own place on the map. */
  onReveal: (nodeId: string) => void
}

/** Rows shown per list before "Show all". */
const PREVIEW = 8

function ConnectionList({
  title,
  items,
  hierarchy,
  onReveal,
}: {
  title: string
  items: readonly Connection[]
  hierarchy: Hierarchy
  onReveal: (nodeId: string) => void
}) {
  const [all, setAll] = useState(false)
  if (items.length === 0) return null
  const shown = all ? items : items.slice(0, PREVIEW)

  return (
    <section className="flex flex-col gap-1.5" aria-label={title}>
      <div className="flex items-baseline gap-2">
        <h4 className="text-xs font-medium">{title}</h4>
        <span className="text-2xs tabular-nums text-muted-foreground">{items.length}</span>
      </div>
      <RowList className="-mx-4">
        {shown.map((item) => {
          const node = hierarchy.byId.get(item.nodeId)
          if (!node) return null
          const Icon = NODE_KIND_META[node.kind].icon
          return (
            <li key={item.nodeId}>
              <Row
                size="md"
                className="px-4"
                leading={<Icon className="text-muted-foreground" />}
                title={
                  <>
                    {node.label}
                    <span className="sr-only">, show it on the map</span>
                  </>
                }
                meta={<span className="truncate">{item.relations.map(relationLabel).join(', ')}</span>}
                trailing={item.count > 1 ? <span className="tabular-nums">×{item.count}</span> : undefined}
                onClick={() => onReveal(item.nodeId)}
              />
            </li>
          )
        })}
      </RowList>
      {items.length > PREVIEW ? (
        <Button variant="ghost" size="xs" className="self-start text-muted-foreground" onClick={() => setAll((value) => !value)}>
          {all ? 'Show fewer' : `Show all ${items.length}`}
        </Button>
      ) : null}
    </section>
  )
}

/**
 * The node on the map: open it when it holds other nodes, and what it is
 * used by and uses, rolled up to its own level (Floorplan's item panel).
 * Every row reveals that neighbour in its place.
 */
export function NodeConnections({ hierarchy, nodeId, onOpen, onReveal }: NodeConnectionsProps) {
  const { usedBy, uses } = useMemo(() => connections(hierarchy, nodeId), [hierarchy, nodeId])
  const inside = descendantCount(hierarchy, nodeId)

  if (inside === 0 && usedBy.length === 0 && uses.length === 0) return null

  return (
    <div className="flex flex-col gap-3">
      {inside > 0 ? (
        <Button variant="outline" size="sm" className="self-start" onClick={() => onOpen(nodeId)}>
          <Maximize2Icon data-icon="inline-start" />
          Open · {inside} inside
        </Button>
      ) : null}
      <ConnectionList title="Used by" items={usedBy} hierarchy={hierarchy} onReveal={onReveal} />
      <ConnectionList title="Uses" items={uses} hierarchy={hierarchy} onReveal={onReveal} />
    </div>
  )
}
