import { graphlib, layout } from '@dagrejs/dagre'

import type { DiagramNode, DiagramSpec } from '../../../api/queries'
import { EDGE_STYLE, edgeStyle } from '../../../lib/labels'
import { GROUP_PAD, GROUP_PAD_TOP, type LaidOutGroup } from '../group-region'
import { EDGE_LABEL_FONT, union, type LaidOutEdge } from '../graph/layout'
import { routeThroughPoints } from '../graph/routing'
import { edgeKey } from '../keys'
import { NEUTRAL_TINT, groupTints, paletteTint, type Tint } from '../palette'
import { fitText, textWidth, type Font } from '../text'
import type { Box, Point } from '../types'
import { parseColumns, type Column } from './columns'

export const TABLE_NAME_FONT: Font = { size: 12, weight: 500, mono: true }
export const COLUMN_FONT: Font = { size: 11, mono: true }
export const TYPE_FONT: Font = { size: 11 }
export const NOTE_FONT: Font = { size: 11 }

export const HEADER_HEIGHT = 28
export const ROW_HEIGHT = 22
export const CARD_PAD_X = 12
export const BADGE_WIDTH = 20
export const BADGE_GAP = 4
/** Gap between a column's name (and badges) and its type. */
export const COLUMN_GAP = 24
const MIN_WIDTH = 170
const MAX_WIDTH = 320

export interface LaidOutColumn extends Column {
  /** Name and type cut to the card width. */
  shownName: string
  shownType: string
}

export interface LaidOutTable {
  node: DiagramNode
  box: Box
  tint: Tint
  name: string
  columns: LaidOutColumn[]
  /** A last line such as "+11 more" or "17 columns"; empty when the columns say it all. */
  note: string
}

export interface ErLayout {
  tables: LaidOutTable[]
  groups: LaidOutGroup[]
  edges: LaidOutEdge[]
  hasPk: boolean
  hasFk: boolean
  dashedEdges: boolean
  content: Box
}

function badgeCount(column: Column) {
  return (column.pk ? 1 : 0) + (column.fk !== null ? 1 : 0)
}

/** Badge space a column's name line needs. */
export function badgesWidth(column: Column) {
  const count = badgeCount(column)
  return count === 0 ? 0 : count * (BADGE_WIDTH + BADGE_GAP)
}

interface SizedTable {
  node: DiagramNode
  w: number
  h: number
  name: string
  columns: LaidOutColumn[]
  note: string
}

/** Card width from the widest row (name + badges + type), rows 22 px, header 28 px. */
function sizeTable(node: DiagramNode): SizedTable {
  const parsed = parseColumns(node.description, node.caption)
  const name = node.label.trim() || node.id
  let note = ''
  if (parsed.columns.length === 0) {
    note = parsed.count !== null ? `${parsed.count} ${parsed.count === 1 ? 'column' : 'columns'}` : 'No columns listed'
  } else if (parsed.more > 0) {
    note = `+${parsed.more} more`
  }

  let rowWidth = textWidth(name, TABLE_NAME_FONT)
  for (const column of parsed.columns) {
    const typeW = column.type ? COLUMN_GAP + textWidth(column.type, TYPE_FONT) : 0
    rowWidth = Math.max(rowWidth, textWidth(column.name, COLUMN_FONT) + badgesWidth(column) + typeW)
  }
  if (note) rowWidth = Math.max(rowWidth, textWidth(note, NOTE_FONT))
  const w = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.ceil(rowWidth + CARD_PAD_X * 2)))
  const inner = w - CARD_PAD_X * 2

  const columns: LaidOutColumn[] = parsed.columns.map((column) => {
    // Only a row wider than MAX_WIDTH is cut: the type yields first (down to 40 px), then the name.
    const typeW = column.type ? textWidth(column.type, TYPE_FONT) : 0
    const nameRoom = inner - badgesWidth(column) - (column.type ? COLUMN_GAP + Math.min(typeW, 40) : 0)
    const shownName = fitText(column.name, Math.max(40, nameRoom), COLUMN_FONT)
    const typeRoom = inner - textWidth(shownName, COLUMN_FONT) - badgesWidth(column) - COLUMN_GAP
    const shownType = column.type ? fitText(column.type, Math.max(40, typeRoom), TYPE_FONT) : ''
    return { ...column, shownName, shownType }
  })

  const rows = columns.length + (note ? 1 : 0)
  return { node, w, h: HEADER_HEIGHT + Math.max(rows, 1) * ROW_HEIGHT, name, columns, note }
}

/**
 * Tables as dagre nodes, left to right so a child table's foreign keys
 * point at the parents on its right; groups (migration directories) as
 * clusters.
 */
export function layoutEr(spec: DiagramSpec): ErLayout {
  const nodes = spec.nodes ?? []
  const groups = spec.groups ?? []
  const edges = spec.edges ?? []
  const tints = groupTints(groups)

  const graph = new graphlib.Graph({ compound: true, multigraph: true })
  graph.setGraph({ rankdir: 'LR', compound: true, nodesep: 40, ranksep: 90, edgesep: 20, marginx: 8, marginy: 8 })
  graph.setDefaultEdgeLabel(() => ({}))

  const groupIds = new Set(groups.map((group) => group.id))
  for (const group of groups) graph.setNode(group.id, { label: group.label })

  const sized = new Map<string, SizedTable>()
  for (const node of nodes) {
    const size = sizeTable(node)
    sized.set(node.id, size)
    graph.setNode(node.id, { width: size.w, height: size.h })
    if (node.groupId && groupIds.has(node.groupId)) graph.setParent(node.id, node.groupId)
  }

  const edgeIds: string[] = []
  edges.forEach((edge, index) => {
    if (!sized.has(edge.source) || !sized.has(edge.target) || edge.source === edge.target) {
      edgeIds.push('')
      return
    }
    const id = edgeKey(edge, index)
    edgeIds.push(id)
    const label = edge.label?.trim() ?? ''
    graph.setEdge(
      edge.source,
      edge.target,
      label ? { width: textWidth(label, EDGE_LABEL_FONT) + 12, height: 18, labelpos: 'c' } : {},
      id,
    )
  })

  layout(graph)

  const tables: LaidOutTable[] = []
  const byId = new Map<string, LaidOutTable>()
  for (const node of nodes) {
    const size = sized.get(node.id)
    const placed = graph.node(node.id)
    if (!size || !placed) continue
    const tint = (node.groupId && tints.get(node.groupId)) || paletteTint(0)
    const item: LaidOutTable = {
      node,
      box: { x: placed.x - size.w / 2, y: placed.y - size.h / 2, w: size.w, h: size.h },
      tint,
      name: size.name,
      columns: size.columns,
      note: size.note,
    }
    tables.push(item)
    byId.set(node.id, item)
  }

  const laidOutGroups: LaidOutGroup[] = []
  for (const group of groups) {
    const inner = union(tables.filter(({ node }) => node.groupId === group.id).map(({ box }) => box))
    if (!inner) continue
    laidOutGroups.push({
      group,
      tint: tints.get(group.id) ?? NEUTRAL_TINT,
      box: { x: inner.x - GROUP_PAD, y: inner.y - GROUP_PAD_TOP, w: inner.w + GROUP_PAD * 2, h: inner.h + GROUP_PAD_TOP + GROUP_PAD },
    })
  }

  const laidOutEdges: LaidOutEdge[] = []
  const labelBoxes: Box[] = []
  let dashedEdges = false
  edges.forEach((edge, index) => {
    const id = edgeIds[index]
    if (!id) return
    const placed = graph.edge(edge.source, edge.target, id) as
      | { points: Point[]; x?: number; y?: number; width?: number; height?: number }
      | undefined
    const source = byId.get(edge.source)
    const target = byId.get(edge.target)
    if (!placed || !source || !target) return
    const label = edge.label?.trim() ?? ''
    const labelAt = label && placed.x !== undefined && placed.y !== undefined ? { x: placed.x, y: placed.y } : null
    if (labelAt && placed.width && placed.height) {
      labelBoxes.push({ x: labelAt.x - placed.width / 2, y: labelAt.y - placed.height / 2, w: placed.width, h: placed.height })
    }
    const style = edgeStyle(edge.style)
    const dashed = Boolean(edge.dashed) || EDGE_STYLE[style].dashed
    if (dashed) dashedEdges = true
    laidOutEdges.push({
      edge,
      id,
      style,
      dashed,
      label,
      route: routeThroughPoints(placed.points, { box: source.box, shape: 'rect' }, { box: target.box, shape: 'rect' }, labelAt),
    })
  })

  const content = union([
    ...tables.map(({ box }) => box),
    ...laidOutGroups.map(({ box }) => box),
    ...laidOutEdges.flatMap(({ route }) => route.points.map((point) => ({ x: point.x, y: point.y, w: 0, h: 0 }))),
    ...labelBoxes,
  ]) ?? { x: 0, y: 0, w: MIN_WIDTH, h: 0 }

  return {
    tables,
    groups: laidOutGroups,
    edges: laidOutEdges,
    hasPk: tables.some((table) => table.columns.some((column) => column.pk)),
    hasFk: tables.some((table) => table.columns.some((column) => column.fk !== null)),
    dashedEdges,
    content,
  }
}
