import { graphlib, layout } from '@dagrejs/dagre'

import type { DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec } from '../../api/queries'
import { EDGE_STYLES, edgeStyle, type EdgeStyle } from '../../lib/labels'
import { routeEdges, type RoutedPath } from './routing'
import type { Box } from './types'

/** Node card: 200 px wide (the 180–220 band of CONTRACT.md §17), two text lines plus an icon. */
export const NODE_WIDTH = 200
export const NODE_HEIGHT = 56

const GROUP_PAD_X = 16
const GROUP_PAD_TOP = 36
const GROUP_PAD_BOTTOM = 16
const CANVAS_MARGIN = 24
export const LEGEND_HEIGHT = 40

export interface LaidOutNode {
  node: DiagramNode
  box: Box
}

export interface LaidOutGroup {
  group: DiagramGroup
  box: Box
}

export interface LaidOutEdge {
  edge: DiagramEdge
  id: string
  style: EdgeStyle
  dashed: boolean
  route: RoutedPath
}

export interface DiagramLayout {
  direction: 'LR' | 'TB'
  nodes: LaidOutNode[]
  groups: LaidOutGroup[]
  edges: LaidOutEdge[]
  /** Edge styles present, in canonical order; a legend is drawn when there are two or more. */
  stylesUsed: EdgeStyle[]
  /** Everything drawn, including the legend strip and an outer margin. */
  bounds: Box
}

function union(boxes: Iterable<Box>): Box | null {
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const box of boxes) {
    minX = Math.min(minX, box.x)
    minY = Math.min(minY, box.y)
    maxX = Math.max(maxX, box.x + box.w)
    maxY = Math.max(maxY, box.y + box.h)
  }
  if (minX === Infinity) return null
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY }
}

/**
 * Compound dagre layout: groups as clusters, nodes as fixed-size cards.
 * Group regions are padded around their children (dagre leaves no room for
 * a label), edges are routed through fanned ports with dagre's dummy points
 * as waypoints.
 */
export function layoutDiagram(spec: DiagramSpec, direction: 'LR' | 'TB'): DiagramLayout {
  const nodes = spec.nodes ?? []
  const groups = spec.groups ?? []
  const edges = spec.edges ?? []

  // Named edges (parallel edges between two nodes keep their own id) need a multigraph.
  const graph = new graphlib.Graph({ compound: true, multigraph: true })
  graph.setGraph({ rankdir: direction, compound: true, nodesep: 32, ranksep: 88, marginx: 8, marginy: 8 })
  graph.setDefaultEdgeLabel(() => ({}))

  const groupIds = new Set(groups.map((group) => group.id))
  for (const group of groups) graph.setNode(group.id, { label: group.label })

  const nodeIds = new Set<string>()
  for (const node of nodes) {
    nodeIds.add(node.id)
    graph.setNode(node.id, { width: NODE_WIDTH, height: NODE_HEIGHT })
    if (node.groupId && groupIds.has(node.groupId)) graph.setParent(node.id, node.groupId)
  }

  const edgeIds: string[] = []
  edges.forEach((edge, index) => {
    if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target) || edge.source === edge.target) {
      edgeIds.push('')
      return
    }
    const id = edge.id || `e${index}`
    edgeIds.push(id)
    graph.setEdge(edge.source, edge.target, {}, id)
  })

  layout(graph)

  const boxes = new Map<string, Box>()
  const laidOutNodes: LaidOutNode[] = nodes.map((node) => {
    const placed = graph.node(node.id)
    const box: Box = { x: placed.x - NODE_WIDTH / 2, y: placed.y - NODE_HEIGHT / 2, w: NODE_WIDTH, h: NODE_HEIGHT }
    boxes.set(node.id, box)
    return { node, box }
  })

  const laidOutGroups: LaidOutGroup[] = []
  for (const group of groups) {
    const children = laidOutNodes.filter(({ node }) => node.groupId === group.id).map(({ box }) => box)
    const inner = union(children)
    if (!inner) continue
    laidOutGroups.push({
      group,
      box: {
        x: inner.x - GROUP_PAD_X,
        y: inner.y - GROUP_PAD_TOP,
        w: inner.w + GROUP_PAD_X * 2,
        h: inner.h + GROUP_PAD_TOP + GROUP_PAD_BOTTOM,
      },
    })
  }

  // Dagre's own edge points (label dummies, cluster border crossings) turn a
  // one-bend elbow into a staircase; the layout is only used for placement
  // and every edge is routed as a lane-shifted elbow between its ports.
  const endpoints = edges.flatMap((edge, index) => {
    const id = edgeIds[index]
    return id ? [{ id, source: edge.source, target: edge.target }] : []
  })
  const routes = routeEdges(endpoints, boxes, direction)

  const laidOutEdges: LaidOutEdge[] = []
  const styles = new Set<EdgeStyle>()
  edges.forEach((edge, index) => {
    const id = edgeIds[index]
    const route = id ? routes.get(id) : undefined
    if (!route) return
    const style = edgeStyle(edge.style)
    styles.add(style)
    laidOutEdges.push({ edge, id, style, dashed: Boolean(edge.dashed), route })
  })
  const stylesUsed = EDGE_STYLES.filter((style) => styles.has(style))

  const content = union([
    ...laidOutNodes.map(({ box }) => box),
    ...laidOutGroups.map(({ box }) => box),
    ...laidOutEdges.flatMap(({ route }) => route.points.map((point) => ({ x: point.x, y: point.y, w: 0, h: 0 }))),
  ]) ?? { x: 0, y: 0, w: NODE_WIDTH, h: NODE_HEIGHT }

  const legend = stylesUsed.length > 1 ? LEGEND_HEIGHT : 0
  const bounds: Box = {
    x: content.x - CANVAS_MARGIN,
    y: content.y - CANVAS_MARGIN,
    w: content.w + CANVAS_MARGIN * 2,
    h: content.h + CANVAS_MARGIN * 2 + legend,
  }

  return { direction, nodes: laidOutNodes, groups: laidOutGroups, edges: laidOutEdges, stylesUsed, bounds }
}
