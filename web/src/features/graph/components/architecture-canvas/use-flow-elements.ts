import { Position } from '@xyflow/react'
import { useMemo } from 'react'

import { NODE_KIND_META } from '../../lib/labels'
import type { GraphEdge, GraphNode, ImpactHighlight } from '../../lib/types'
import { NODE_HEIGHT, NODE_WIDTH, layoutGraph, type Point } from './layout'
import type { ArchEdge, ArchNode, NodeEmphasis } from './types'

interface FlowElementsInput {
  nodes: GraphNode[]
  edges: GraphEdge[]
  selectedId: string | null
  highlight: ImpactHighlight | null
}

function emphasisFor(id: string, highlight: ImpactHighlight | null): NodeEmphasis {
  if (!highlight) return 'none'
  if (id === highlight.originId) return 'origin'
  if (highlight.ids.has(id)) return 'impacted'
  return 'dimmed'
}

function inHighlight(id: string, highlight: ImpactHighlight) {
  return id === highlight.originId || highlight.ids.has(id)
}

/**
 * Turns contract nodes/edges into positioned React Flow elements. The layout
 * only reruns when the visible set changes; selection and highlight are cheap
 * re-maps over the same positions.
 */
export function useFlowElements({ nodes, edges, selectedId, highlight }: FlowElementsInput) {
  const positions = useMemo<Map<string, Point>>(
    () =>
      layoutGraph(
        nodes.map((node) => node.id),
        edges.map((edge) => ({ source: edge.sourceId, target: edge.targetId })),
      ),
    [nodes, edges],
  )

  const flowNodes = useMemo<ArchNode[]>(
    () =>
      nodes.map((node) => ({
        id: node.id,
        type: node.kind,
        position: positions.get(node.id) ?? { x: 0, y: 0 },
        width: NODE_WIDTH,
        height: NODE_HEIGHT,
        sourcePosition: Position.Right,
        targetPosition: Position.Left,
        selected: node.id === selectedId,
        draggable: false,
        connectable: false,
        ariaLabel: `${node.label}, ${NODE_KIND_META[node.kind].label}`,
        data: { label: node.label, kind: node.kind, emphasis: emphasisFor(node.id, highlight) },
      })),
    [nodes, positions, selectedId, highlight],
  )

  const flowEdges = useMemo<ArchEdge[]>(
    () =>
      edges.map((edge) => ({
        id: edge.id,
        type: 'relation',
        source: edge.sourceId,
        target: edge.targetId,
        selectable: false,
        focusable: false,
        ariaLabel: `${edge.relation.toLowerCase().replaceAll('_', ' ')}`,
        data: {
          relation: edge.relation,
          category: edge.category,
          expired: edge.validTo != null,
          dimmed: highlight ? !(inHighlight(edge.sourceId, highlight) && inHighlight(edge.targetId, highlight)) : false,
        },
      })),
    [edges, highlight],
  )

  return { positions, flowNodes, flowEdges }
}
