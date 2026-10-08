import { graphlib, layout } from '@dagrejs/dagre'

/** Node box in pixels. The node component renders exactly this size so edges meet its border. */
export const NODE_WIDTH = 176
export const NODE_HEIGHT = 44

export interface LayoutLink {
  source: string
  target: string
}

export interface Point {
  x: number
  y: number
}

/** Left-to-right layered layout. Returns the top-left corner of every node. */
export function layoutGraph(nodeIds: readonly string[], links: readonly LayoutLink[]): Map<string, Point> {
  const graph = new graphlib.Graph()
  graph.setGraph({ rankdir: 'LR', nodesep: 20, ranksep: 72, marginx: 8, marginy: 8 })
  graph.setDefaultEdgeLabel(() => ({}))

  const known = new Set(nodeIds)
  for (const id of nodeIds) {
    graph.setNode(id, { width: NODE_WIDTH, height: NODE_HEIGHT })
  }
  for (const link of links) {
    if (known.has(link.source) && known.has(link.target) && link.source !== link.target) {
      graph.setEdge(link.source, link.target)
    }
  }

  layout(graph)

  const positions = new Map<string, Point>()
  for (const id of nodeIds) {
    const node = graph.node(id)
    positions.set(id, { x: node.x - NODE_WIDTH / 2, y: node.y - NODE_HEIGHT / 2 })
  }
  return positions
}
