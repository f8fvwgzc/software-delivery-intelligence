import { useQueryClient } from '@tanstack/react-query'

import { graphQueries } from '../../api/queries'
import { NODE_KIND_META, hopsLabel, relationLabel } from '../../lib/labels'
import type { GraphNode, ImpactedNode } from '../../lib/types'

interface ImpactItemProps {
  projectId: string
  node: GraphNode
  via: string[]
  onSelect: (id: string) => void
}

function ImpactItem({ projectId, node, via, onSelect }: ImpactItemProps) {
  const queryClient = useQueryClient()
  const meta = NODE_KIND_META[node.kind]
  const Icon = meta.icon

  // The next click is usually "what does *this* one touch": warm it on intent.
  const prefetch = () => {
    void queryClient.prefetchQuery(graphQueries.impact(projectId, node.id))
  }

  return (
    <li>
      <button
        type="button"
        onClick={() => onSelect(node.id)}
        onMouseEnter={prefetch}
        onFocus={prefetch}
        className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left transition-colors duration-150 hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none motion-reduce:transition-none"
      >
        <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground">
          <Icon className="size-3" />
        </span>
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="flex items-baseline gap-1.5">
            <span className="truncate text-xs font-medium">{node.label}</span>
            <span className="shrink-0 text-2xs text-muted-foreground">{meta.label}</span>
          </span>
          {via.length > 0 ? (
            <span className="truncate font-mono text-2xs text-muted-foreground">{via.map(relationLabel).join(' › ')}</span>
          ) : null}
        </span>
      </button>
    </li>
  )
}

interface ImpactGroupProps {
  projectId: string
  distance: number
  items: ImpactedNode[]
  onSelect: (id: string) => void
}

/** Every node reached at one distance from the origin. */
export function ImpactGroup({ projectId, distance, items, onSelect }: ImpactGroupProps) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-baseline justify-between px-2">
        <h4 className="text-2xs font-medium text-muted-foreground">{hopsLabel(distance)}</h4>
        <span className="text-2xs tabular-nums text-muted-foreground">{items.length}</span>
      </div>
      <ul className="flex flex-col">
        {items.map((item) =>
          item.node ? (
            <ImpactItem key={item.node.id} projectId={projectId} node={item.node} via={item.via} onSelect={onSelect} />
          ) : null,
        )}
      </ul>
    </div>
  )
}
