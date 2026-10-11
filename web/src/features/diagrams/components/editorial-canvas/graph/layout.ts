import { graphlib, layout } from '@dagrejs/dagre'

import type { DiagramEdge, DiagramGroup, DiagramNode, DiagramSpec } from '../../../api/queries'
import { EDGE_STYLE, NODE_SHAPES, edgeStyle, nodeShape, type EdgeStyle, type NodeShape } from '../../../lib/labels'
import { GROUP_PAD, GROUP_PAD_TOP, type LaidOutGroup } from '../group-region'
import { NEUTRAL_TINT, groupTints, nodeTint, type Tint } from '../palette'
import { fitText, linesWidth, textWidth, wrapText, type Font } from '../text'
import { edgeKey } from '../keys'
import type { Box, Direction, Point } from '../types'
import { layoutBands } from './bands'
import { shapeInset, shapeTopInset } from './node-shape'
import { routeOrthogonal, type OrthoNode } from './ortho'
import { routeThroughPoints, type RoutedPath } from './routing'

export const LABEL_FONT: Font = { size: 13, weight: 500 }
export const CIRCLE_FONT: Font = { size: 11, weight: 500 }
export const SUB_FONT: Font = { size: 11, mono: true }
export const EDGE_LABEL_FONT: Font = { size: 12 }

const MIN_WIDTH = 150
/** Labels wrap past this; the node grows to the longest line. */
const MAX_TEXT_WIDTH = 200
const PAD_X = 14
const PAD_Y = 12
const LABEL_LINE = 16
const SUB_LINE = 14
const CIRCLE_DIAMETER = 64
const EDGE_LABEL_HEIGHT = 18
const EDGE_LABEL_PAD = 6

export interface LaidOutNode {
  node: DiagramNode
  box: Box
  shape: NodeShape
  tint: Tint
  /** The label, wrapped to at most two lines. */
  lines: string[]
  /** The mono line under the label: `[file.rs]` or the caption; empty when there is neither. */
  sub: string
  /** Horizontal and top room the shape's outline takes from the text. */
  inset: { x: number; top: number }
}

export interface LaidOutEdge {
  edge: DiagramEdge
  id: string
  style: EdgeStyle
  dashed: boolean
  label: string
  route: RoutedPath
}

export interface GraphLayout {
  direction: Direction
  nodes: LaidOutNode[]
  groups: LaidOutGroup[]
  edges: LaidOutEdge[]
  /** Shapes drawn, in legend order. */
  shapesUsed: NodeShape[]
  /** Whether any edge is solid, and whether any is dashed. */
  solidEdges: boolean
  dashedEdges: boolean
  /** Everything drawn, without margin. */
  content: Box
}

export function union(boxes: Iterable<Box>): Box | null {
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

function fileName(path: string) {
  const trimmed = path.replace(/\/+$/, '')
  return trimmed.slice(trimmed.lastIndexOf('/') + 1)
}

/** Code nodes show the file that implements them; data and external nodes their caption (engine, image, vendor). */
function secondLine(node: DiagramNode): string {
  const caption = node.caption?.trim() ?? ''
  const file = node.path ? `[${fileName(node.path)}]` : ''
  const codeLike = node.kind !== 'datastore' && node.kind !== 'queue' && node.kind !== 'external'
  return codeLike ? file || caption : caption || file
}

interface SizedNode {
  node: DiagramNode
  shape: NodeShape
  w: number
  h: number
  lines: string[]
  sub: string
  inset: { x: number; top: number }
}

/** Measures a node: minimum 150 px wide, as wide as its longest line, two text lines tall. */
function sizeNode(node: DiagramNode): SizedNode {
  const shape = nodeShape(node.kind)
  if (shape === 'circle') {
    const lines = wrapText(node.label, CIRCLE_DIAMETER - 14, CIRCLE_FONT)
    return { node, shape, w: CIRCLE_DIAMETER, h: CIRCLE_DIAMETER, lines, sub: '', inset: { x: 0, top: 0 } }
  }
  const lines = wrapText(node.label, MAX_TEXT_WIDTH, LABEL_FONT)
  const rawSub = secondLine(node)
  const textH = lines.length * LABEL_LINE + (rawSub ? SUB_LINE : 0)
  const h = textH + PAD_Y * 2 + shapeTopInset(shape, textH + PAD_Y * 2)
  const insetX = shapeInset(shape, h)
  const textW = Math.max(linesWidth(lines, LABEL_FONT), Math.min(textWidth(rawSub, SUB_FONT), MAX_TEXT_WIDTH))
  const w = Math.max(MIN_WIDTH, Math.ceil(textW + (PAD_X + insetX) * 2))
  const sub = rawSub ? fitText(rawSub, w - (PAD_X + insetX) * 2, SUB_FONT) : ''
  return { node, shape, w, h, lines, sub, inset: { x: insetX, top: shapeTopInset(shape, h) } }
}

interface Placed {
  boxes: Map<string, Box>
  groupBoxes: Map<string, Box>
  routes: Map<string, RoutedPath>
  /** Dagre's label positions, when the layout reserved room for them. */
  labelBoxes: Box[]
}

interface EdgeEntry {
  edge: DiagramEdge
  id: string
  label: string
}

/**
 * Stacked bands (TB): actors on top, groups in role order, rows of at
 * most four nodes, orthogonal routing through the channels and corridors
 * between them (see bands.ts and ortho.ts).
 */
function placeStacked(nodes: readonly DiagramNode[], groups: readonly DiagramGroup[], edges: readonly DiagramEdge[], sized: ReadonlyMap<string, SizedNode>, entries: readonly EdgeEntry[]): Placed {
  const bands = layoutBands(nodes, groups, edges, sized)
  const groupBoxes = new Map<string, Box>()
  for (const band of bands.bands) for (const placed of band.groups) groupBoxes.set(placed.group.id, placed.box)
  const orthoNodes = new Map<string, OrthoNode>()
  for (const [id, box] of bands.boxes) {
    const size = sized.get(id)
    if (size) orthoNodes.set(id, { box, shape: size.shape })
  }
  const routes = routeOrthogonal(
    entries.map(({ edge, id, label }) => ({
      id,
      source: edge.source,
      target: edge.target,
      labelWidth: label ? textWidth(label, EDGE_LABEL_FONT) + EDGE_LABEL_PAD * 2 : 0,
    })),
    orthoNodes,
    bands.rows,
    [...groupBoxes.values()].map((box) => ({ x: box.x, y: box.y, w: box.w, h: GROUP_PAD_TOP })),
  )
  return { boxes: bands.boxes, groupBoxes, routes, labelBoxes: [] }
}

/**
 * Compound dagre layout (LR): groups as clusters, edge labels as dagre
 * label dummies so the path passes through them, dagre's own edge points
 * kept as rounded polylines.
 */
function placeDagre(nodes: readonly DiagramNode[], groups: readonly DiagramGroup[], sized: ReadonlyMap<string, SizedNode>, entries: readonly EdgeEntry[], direction: Direction): Placed {
  const graph = new graphlib.Graph({ compound: true, multigraph: true })
  graph.setGraph({ rankdir: direction, compound: true, nodesep: 40, ranksep: 70, edgesep: 20, marginx: 8, marginy: 8 })
  graph.setDefaultEdgeLabel(() => ({}))
  const groupIds = new Set(groups.map((group) => group.id))
  for (const group of groups) graph.setNode(group.id, { label: group.label })
  for (const node of nodes) {
    const size = sized.get(node.id)
    if (!size) continue
    graph.setNode(node.id, { width: size.w, height: size.h })
    if (node.groupId && groupIds.has(node.groupId)) graph.setParent(node.id, node.groupId)
  }
  for (const { edge, id, label } of entries) {
    graph.setEdge(
      edge.source,
      edge.target,
      label ? { width: textWidth(label, EDGE_LABEL_FONT) + EDGE_LABEL_PAD * 2, height: EDGE_LABEL_HEIGHT, labelpos: 'c' } : {},
      id,
    )
  }
  layout(graph)

  const boxes = new Map<string, Box>()
  for (const node of nodes) {
    const size = sized.get(node.id)
    const placed = graph.node(node.id)
    if (size && placed) boxes.set(node.id, { x: placed.x - size.w / 2, y: placed.y - size.h / 2, w: size.w, h: size.h })
  }
  const groupBoxes = new Map<string, Box>()
  for (const group of groups) {
    const inner = union(nodes.filter((node) => node.groupId === group.id).flatMap((node) => boxes.get(node.id) ?? []))
    if (inner) {
      groupBoxes.set(group.id, { x: inner.x - GROUP_PAD, y: inner.y - GROUP_PAD_TOP, w: inner.w + GROUP_PAD * 2, h: inner.h + GROUP_PAD_TOP + GROUP_PAD })
    }
  }
  const routes = new Map<string, RoutedPath>()
  const labelBoxes: Box[] = []
  for (const { edge, id, label } of entries) {
    const placed = graph.edge(edge.source, edge.target, id) as
      | { points: Point[]; x?: number; y?: number; width?: number; height?: number }
      | undefined
    const source = boxes.get(edge.source)
    const target = boxes.get(edge.target)
    const sourceSize = sized.get(edge.source)
    const targetSize = sized.get(edge.target)
    if (!placed || !source || !target || !sourceSize || !targetSize) continue
    const labelAt = label && placed.x !== undefined && placed.y !== undefined ? { x: placed.x, y: placed.y } : null
    if (labelAt && placed.width && placed.height) {
      labelBoxes.push({ x: labelAt.x - placed.width / 2, y: labelAt.y - placed.height / 2, w: placed.width, h: placed.height })
    }
    routes.set(id, routeThroughPoints(placed.points, { box: source, shape: sourceSize.shape }, { box: target, shape: targetSize.shape }, labelAt))
  }
  return { boxes, groupBoxes, routes, labelBoxes }
}

/** Sizes every node from its text, places nodes and groups by direction, routes and classifies the edges. */
export function layoutGraph(spec: DiagramSpec, direction: Direction): GraphLayout {
  const nodes = spec.nodes ?? []
  const groups = spec.groups ?? []
  const edges = spec.edges ?? []
  const tints = groupTints(groups)

  const sized = new Map<string, SizedNode>()
  for (const node of nodes) sized.set(node.id, sizeNode(node))

  const entries: EdgeEntry[] = []
  edges.forEach((edge, index) => {
    if (!sized.has(edge.source) || !sized.has(edge.target) || edge.source === edge.target) return
    entries.push({ edge, id: edgeKey(edge, index), label: edge.label?.trim() ?? '' })
  })

  const placed =
    direction === 'TB' ? placeStacked(nodes, groups, edges, sized, entries) : placeDagre(nodes, groups, sized, entries, direction)

  const laidOutNodes: LaidOutNode[] = []
  for (const node of nodes) {
    const size = sized.get(node.id)
    const box = placed.boxes.get(node.id)
    if (!size || !box) continue
    laidOutNodes.push({ node, box, shape: size.shape, tint: nodeTint(node, tints), lines: size.lines, sub: size.sub, inset: size.inset })
  }

  const laidOutGroups: LaidOutGroup[] = []
  for (const group of groups) {
    const box = placed.groupBoxes.get(group.id)
    if (box) laidOutGroups.push({ group, box, tint: tints.get(group.id) ?? NEUTRAL_TINT })
  }

  const laidOutEdges: LaidOutEdge[] = []
  let solidEdges = false
  let dashedEdges = false
  for (const { edge, id, label } of entries) {
    const route = placed.routes.get(id)
    if (!route) continue
    const style = edgeStyle(edge.style)
    const dashed = Boolean(edge.dashed) || EDGE_STYLE[style].dashed
    if (dashed) dashedEdges = true
    else solidEdges = true
    laidOutEdges.push({ edge, id, style, dashed, label, route })
  }

  const shapes = new Set(laidOutNodes.map((item) => item.shape))
  const content = union([
    ...laidOutNodes.map(({ box }) => box),
    ...laidOutGroups.map(({ box }) => box),
    ...laidOutEdges.flatMap(({ route }) => route.points.map((point) => ({ x: point.x, y: point.y, w: 0, h: 0 }))),
    ...laidOutEdges.flatMap(({ route, label }) =>
      label ? [{ x: route.mid.x - textWidth(label, EDGE_LABEL_FONT) / 2 - EDGE_LABEL_PAD, y: route.mid.y - EDGE_LABEL_HEIGHT / 2, w: textWidth(label, EDGE_LABEL_FONT) + EDGE_LABEL_PAD * 2, h: EDGE_LABEL_HEIGHT }] : [],
    ),
    ...placed.labelBoxes,
  ]) ?? { x: 0, y: 0, w: MIN_WIDTH, h: 0 }

  return {
    direction,
    nodes: laidOutNodes,
    groups: laidOutGroups,
    edges: laidOutEdges,
    shapesUsed: NODE_SHAPES.filter((shape) => shapes.has(shape)),
    solidEdges,
    dashedEdges,
    content,
  }
}
