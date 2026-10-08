import type { Edge, Node } from '@xyflow/react'

import type { GraphEdge, GraphNode, ImpactHighlight, NodeKind, RelationCategory } from '../../lib/types'

/** How a node is drawn relative to the current impact analysis. */
export type NodeEmphasis = 'none' | 'origin' | 'impacted' | 'dimmed'

export interface ArchNodeData extends Record<string, unknown> {
  label: string
  kind: NodeKind
  emphasis: NodeEmphasis
}

export interface ArchEdgeData extends Record<string, unknown> {
  relation: string
  category: RelationCategory
  /** `validTo` is set: the relation no longer holds. */
  expired: boolean
  /** Neither end is part of the current impact set. */
  dimmed: boolean
}

/** The node `type` is its kind, so one node component is registered per kind. */
export type ArchNode = Node<ArchNodeData, NodeKind>
export type ArchEdge = Edge<ArchEdgeData, 'relation'>

/** Imperative surface the toolbar and the details panel use. */
export interface CanvasHandle {
  fitView: () => void
  focusNode: (id: string) => void
}

export interface ArchitectureCanvasProps {
  nodes: GraphNode[]
  edges: GraphEdge[]
  selectedId: string | null
  highlight: ImpactHighlight | null
  onSelect: (id: string | null) => void
  ref?: React.Ref<CanvasHandle>
}
