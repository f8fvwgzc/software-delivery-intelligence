import { graphlib, layout } from '@dagrejs/dagre'

import type { DiagramEdge, DiagramGroup, DiagramNode } from '../../../api/queries'
import { GROUP_PAD, GROUP_PAD_TOP } from '../group-region'
import type { Box } from '../types'

/**
 * Top-down band layout: actors in a top band, then the groups stacked in
 * role order, each group's nodes wrapped into rows of at most four so
 * the drawing grows down, never sideways (gitdiagram's shape).
 */

/** Nodes per row inside a group, and the widest a row may get. */
const ROW_MAX_NODES = 4
const ROW_MAX_WIDTH = 900
/** Horizontal gap between nodes in a row (dagre's nodesep). */
export const NODE_GAP = 40
/** Vertical gap between rows inside a group: a channel for the edges between them. */
export const ROW_GAP = 60
/** Vertical gap between bands (groups): a wider channel, long edges and their labels run here. */
export const BAND_GAP = 104
/** Gap between the actor band and the first group. */
const ACTOR_GAP = 88
/** Gap between groups sharing a band. */
const GROUP_GAP = 40
/** Single-row groups share a band while they fit in this width together. */
const BAND_MAX_WIDTH = 960
/** Groups matching no role sit between execution and integrations. */
const UNMATCHED_ROLE = 2.5

/** Role order of groups, matched on id and label; unmatched groups follow in spec order. */
const ROLES: readonly (readonly string[])[] = [
  ['bootstrap', 'config', 'entry', 'boot', 'setup', 'cli'],
  ['execution', 'core', 'runtime', 'domain', 'engine', 'worker', 'pipeline'],
  ['integration', 'adapter', 'client', 'external', 'vendor', 'provider'],
  ['monitor', 'observ', 'ui', 'api', 'http', 'web', 'interface', 'frontend', 'gateway'],
  ['state', 'storage', 'store', 'persist', 'history', 'data', 'database', 'db'],
]

export interface NodeSize {
  w: number
  h: number
}

export interface BandRow {
  /** Node ids left to right. */
  ids: string[]
  y: number
  h: number
}

export interface PlacedGroup {
  group: DiagramGroup
  /** The region behind the group's nodes, padded. */
  box: Box
}

export interface Band {
  /** Groups drawn side by side in this band; empty for the actor band and ungrouped nodes. */
  groups: PlacedGroup[]
  rows: BandRow[]
  box: Box
}

export interface BandLayout {
  bands: Band[]
  boxes: Map<string, Box>
  /** Every row top-down across all bands, for the edge router's obstacle map. */
  rows: BandRow[]
  /** Width of the widest band; the bands are centred on `axis`. */
  axis: number
}

function roleOf(group: DiagramGroup): number | null {
  const text = `${group.id} ${group.label}`.toLowerCase()
  const index = ROLES.findIndex((keywords) => keywords.some((keyword) => text.includes(keyword)))
  return index === -1 ? null : index
}

/** Groups in role order; without any role match, in topological order of the edges between them. */
function orderGroups(groups: readonly DiagramGroup[], nodes: readonly DiagramNode[], edges: readonly DiagramEdge[]): DiagramGroup[] {
  const roles = new Map(groups.map((group) => [group.id, roleOf(group)]))
  if (groups.some((group) => roles.get(group.id) !== null)) {
    return [...groups].sort((a, b) => (roles.get(a.id) ?? UNMATCHED_ROLE) - (roles.get(b.id) ?? UNMATCHED_ROLE))
  }
  // Net edge direction between two groups says which comes first; Kahn's order, ties by spec order.
  const groupOf = new Map(nodes.map((node) => [node.id, node.groupId ?? '']))
  const count = new Map<string, number>()
  for (const edge of edges) {
    const a = groupOf.get(edge.source)
    const b = groupOf.get(edge.target)
    if (!a || !b || a === b) continue
    count.set(`${a}>${b}`, (count.get(`${a}>${b}`) ?? 0) + 1)
  }
  const before = new Map<string, Set<string>>(groups.map((group) => [group.id, new Set()]))
  for (const a of groups) {
    for (const b of groups) {
      if (a === b) continue
      if ((count.get(`${a.id}>${b.id}`) ?? 0) > (count.get(`${b.id}>${a.id}`) ?? 0)) before.get(b.id)?.add(a.id)
    }
  }
  const ordered: DiagramGroup[] = []
  const remaining = [...groups]
  while (remaining.length > 0) {
    const index = remaining.findIndex((group) => [...(before.get(group.id) ?? [])].every((id) => ordered.some((done) => done.id === id)))
    const [next] = remaining.splice(index === -1 ? 0 : index, 1)
    ordered.push(next)
  }
  return ordered
}

/**
 * Order inside a group: a dagre pass over the group's own nodes and edges
 * gives sources first and keeps callers near callees; datastores go last so
 * they land on the bottom row.
 */
function orderNodes(ids: readonly string[], nodes: ReadonlyMap<string, DiagramNode>, edges: readonly DiagramEdge[], sizes: ReadonlyMap<string, NodeSize>): string[] {
  if (ids.length <= 1) return [...ids]
  const graph = new graphlib.Graph({ multigraph: true })
  graph.setGraph({ rankdir: 'TB', nodesep: NODE_GAP, ranksep: ROW_GAP })
  graph.setDefaultEdgeLabel(() => ({}))
  const members = new Set(ids)
  for (const id of ids) graph.setNode(id, { width: sizes.get(id)?.w ?? 150, height: sizes.get(id)?.h ?? 52 })
  edges.forEach((edge, index) => {
    if (members.has(edge.source) && members.has(edge.target) && edge.source !== edge.target) {
      graph.setEdge(edge.source, edge.target, {}, `e${index}`)
    }
  })
  layout(graph)
  const position = new Map(ids.map((id) => [id, graph.node(id)]))
  return [...ids].sort((a, b) => {
    const da = nodes.get(a)?.kind === 'datastore' ? 1 : 0
    const db = nodes.get(b)?.kind === 'datastore' ? 1 : 0
    if (da !== db) return da - db
    const pa = position.get(a)
    const pb = position.get(b)
    return (pa?.y ?? 0) - (pb?.y ?? 0) || (pa?.x ?? 0) - (pb?.x ?? 0)
  })
}

/** Cuts an ordered list into rows of at most four nodes and at most ~900 px. */
function wrapRows(ids: readonly string[], sizes: ReadonlyMap<string, NodeSize>): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let width = 0
  for (const id of ids) {
    const w = sizes.get(id)?.w ?? 150
    const next = row.length === 0 ? w : width + NODE_GAP + w
    if (row.length > 0 && (row.length >= ROW_MAX_NODES || next > ROW_MAX_WIDTH)) {
      rows.push(row)
      row = []
      width = w
    } else {
      width = next
    }
    row.push(id)
  }
  if (row.length > 0) rows.push(row)
  return rows
}

function rowWidth(ids: readonly string[], sizes: ReadonlyMap<string, NodeSize>) {
  return ids.reduce((sum, id, index) => sum + (sizes.get(id)?.w ?? 150) + (index > 0 ? NODE_GAP : 0), 0)
}

function rowHeight(ids: readonly string[], sizes: ReadonlyMap<string, NodeSize>) {
  return ids.reduce((max, id) => Math.max(max, sizes.get(id)?.h ?? 52), 0)
}

export function layoutBands(
  nodes: readonly DiagramNode[],
  groups: readonly DiagramGroup[],
  edges: readonly DiagramEdge[],
  sizes: ReadonlyMap<string, NodeSize>,
): BandLayout {
  const byId = new Map(nodes.map((node) => [node.id, node]))
  const groupIds = new Set(groups.map((group) => group.id))
  const actors = nodes.filter((node) => node.kind === 'actor' && !(node.groupId && groupIds.has(node.groupId)))
  const loose = nodes.filter((node) => node.kind !== 'actor' && !(node.groupId && groupIds.has(node.groupId)))

  // Row lists per band, before any positions. Consecutive single-row groups
  // share a band while they fit side by side, so a small diagram does not
  // become a thin tower.
  interface Plan {
    groups: { group: DiagramGroup; rows: string[][] }[]
    /** Rows for a band without groups (actors, ungrouped nodes). */
    rows: string[][]
    actors: boolean
  }
  const plan: Plan[] = []
  if (actors.length > 0) plan.push({ groups: [], rows: [actors.map((node) => node.id)], actors: true })
  if (loose.length > 0) plan.push({ groups: [], rows: wrapRows(orderNodes(loose.map((node) => node.id), byId, edges, sizes), sizes), actors: false })
  const groupWidth = (rows: string[][]) => rows.reduce((max, row) => Math.max(max, rowWidth(row, sizes)), 0) + GROUP_PAD * 2
  const planWidth = (item: Plan) =>
    item.groups.length > 0
      ? item.groups.reduce((sum, g, index) => sum + groupWidth(g.rows) + (index > 0 ? GROUP_GAP : 0), 0)
      : item.rows.reduce((max, row) => Math.max(max, rowWidth(row, sizes)), 0)
  for (const group of orderGroups(groups, nodes, edges)) {
    const ids = nodes.filter((node) => node.groupId === group.id).map((node) => node.id)
    if (ids.length === 0) continue
    const rows = wrapRows(orderNodes(ids, byId, edges, sizes), sizes)
    const last = plan[plan.length - 1]
    const shareable =
      last && last.groups.length > 0 && rows.length === 1 && last.groups.every((g) => g.rows.length === 1)
    if (shareable && planWidth(last) + GROUP_GAP + groupWidth(rows) <= BAND_MAX_WIDTH) {
      last.groups.push({ group, rows })
    } else {
      plan.push({ groups: [{ group, rows }], rows: [], actors: false })
    }
  }

  const widest = plan.reduce((max, item) => Math.max(max, planWidth(item)), 0)
  const axis = widest / 2

  const boxes = new Map<string, Box>()
  const allRows: BandRow[] = []
  const bands: Band[] = []
  let y = 0
  for (const item of plan) {
    const top = y
    const width = planWidth(item)
    const left = axis - width / 2
    const rows: BandRow[] = []
    const placedGroups: PlacedGroup[] = []
    let bottom = top

    if (item.groups.length === 0) {
      let rowY = top
      item.rows.forEach((ids, rowIndex) => {
        const h = rowHeight(ids, sizes)
        let x = axis - rowWidth(ids, sizes) / 2
        for (const id of ids) {
          const size = sizes.get(id) ?? { w: 150, h: 52 }
          boxes.set(id, { x, y: rowY + (h - size.h) / 2, w: size.w, h: size.h })
          x += size.w + NODE_GAP
        }
        const row = { ids: [...ids], y: rowY, h }
        rows.push(row)
        allRows.push(row)
        rowY += h + (rowIndex < item.rows.length - 1 ? ROW_GAP : 0)
      })
      bottom = rowY
    } else if (item.groups.length === 1) {
      const { group, rows: groupRows } = item.groups[0]
      let rowY = top + GROUP_PAD_TOP
      groupRows.forEach((ids, rowIndex) => {
        const h = rowHeight(ids, sizes)
        let x = axis - rowWidth(ids, sizes) / 2
        for (const id of ids) {
          const size = sizes.get(id) ?? { w: 150, h: 52 }
          boxes.set(id, { x, y: rowY + (h - size.h) / 2, w: size.w, h: size.h })
          x += size.w + NODE_GAP
        }
        const row = { ids: [...ids], y: rowY, h }
        rows.push(row)
        allRows.push(row)
        rowY += h + (rowIndex < groupRows.length - 1 ? ROW_GAP : 0)
      })
      bottom = rowY + GROUP_PAD
      placedGroups.push({ group, box: { x: left, y: top, w: width, h: bottom - top } })
    } else {
      // Several single-row groups side by side: one shared row across them.
      const ids = item.groups.flatMap((g) => g.rows[0])
      const h = rowHeight(ids, sizes)
      const rowY = top + GROUP_PAD_TOP
      let x = left
      for (const { group, rows: groupRows } of item.groups) {
        const start = x
        x += GROUP_PAD
        for (const id of groupRows[0]) {
          const size = sizes.get(id) ?? { w: 150, h: 52 }
          boxes.set(id, { x, y: rowY + (h - size.h) / 2, w: size.w, h: size.h })
          x += size.w + NODE_GAP
        }
        x += GROUP_PAD - NODE_GAP
        placedGroups.push({ group, box: { x: start, y: top, w: x - start, h: GROUP_PAD_TOP + h + GROUP_PAD } })
        x += GROUP_GAP
      }
      const row = { ids, y: rowY, h }
      rows.push(row)
      allRows.push(row)
      bottom = rowY + h + GROUP_PAD
    }

    const box: Box = { x: left, y: top, w: width, h: bottom - top }
    bands.push({ groups: placedGroups, rows, box })
    y = bottom + (item.actors ? ACTOR_GAP : BAND_GAP)
  }

  return { bands, boxes, rows: allRows, axis }
}
