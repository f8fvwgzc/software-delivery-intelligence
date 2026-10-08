import type { Graph, GraphEdge, GraphNode, NodeKind } from './types'

export interface VisibleGraph {
  visibleNodes: GraphNode[]
  visibleEdges: GraphEdge[]
}

/** Nodes of the chosen kinds whose label or key matches; edges between what remains. */
export function filterGraph(graph: Graph, kinds: readonly NodeKind[], search: string): VisibleGraph {
  const kindSet = new Set(kinds)
  const query = search.trim().toLowerCase()

  const visibleNodes = graph.nodes.filter(
    (node) =>
      kindSet.has(node.kind) &&
      (query === '' || node.label.toLowerCase().includes(query) || node.key.toLowerCase().includes(query)),
  )
  const ids = new Set(visibleNodes.map((node) => node.id))
  const visibleEdges = graph.edges.filter((edge) => ids.has(edge.sourceId) && ids.has(edge.targetId))

  return { visibleNodes, visibleEdges }
}
