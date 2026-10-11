import type { NodeShape } from '../../../lib/labels'
import type { Box, Point } from '../types'
import type { BandRow } from './bands'
import { routeThroughPoints, type RoutedPath } from './routing'

/**
 * Orthogonal routing for the band layout. Every row of nodes is an
 * obstacle; the gaps between rows are horizontal channels and the gaps
 * between nodes (or the margins outside the drawing) are vertical
 * corridors. An edge leaves its node from the top or bottom, runs along
 * a channel, drops through a corridor when it has rows to cross, and
 * enters its target from the top or bottom, so runs are vertical wherever
 * they can be and never cut through a node. Runs that share a channel or
 * corridor take separate lanes.
 */

export interface OrthoEdge {
  id: string
  source: string
  target: string
  /** Width the label needs, 0 when there is none. */
  labelWidth: number
}

export interface OrthoNode {
  box: Box
  shape: NodeShape
}

const LANE_GAP = 10
const PORT_INSET = 12
/** Outer corridors start this far outside the widest row they pass. */
const MARGIN_CORRIDOR = 28
/** Space a channel keeps between the outermost lane and the rows it lies between. */
const CHANNEL_INSET = 12
const LABEL_HEIGHT = 18
const LABEL_NUDGE = 14

type Side = 'top' | 'bottom' | 'left' | 'right'

interface Run {
  edgeId: string
  /** Extent along the channel (x) or corridor (y). */
  from: number
  to: number
  lane: number
  /** The run's fixed coordinate once lanes are assigned: y in a channel, x in a corridor. */
  pos: number
}

interface Channel {
  /** Centre line and the free span around it. */
  centre: number
  span: number
  runs: Run[]
}

interface Corridor {
  /** Fixed x for a gap between nodes; a margin corridor grows outward from `edge`. */
  kind: 'gap' | 'left' | 'right'
  x: number
  runs: Run[]
}

interface Plan {
  edge: OrthoEdge
  sourceSide: Side
  targetSide: Side
  /** Channel indices the route runs along; equal when no row is crossed, empty for a direct sideways edge. */
  channelA: number
  channelB: number
  corridor: Corridor | null
  corridorRun: Run | null
  runA: Run | null
  runB: Run | null
}

function boxesOverlap(a: Box, b: Box) {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

function overlaps(a: Run, b: Run) {
  return Math.min(a.from, a.to) < Math.max(b.from, b.to) && Math.min(b.from, b.to) < Math.max(a.from, a.to)
}

/** Greedy interval colouring: each run gets the lowest lane no overlapping run already holds. Returns the lane count. */
function assignLanes(runs: Run[]): number {
  const sorted = [...runs].sort((a, b) => Math.min(a.from, a.to) - Math.min(b.from, b.to))
  let count = 0
  for (const run of sorted) {
    const taken = new Set<number>()
    for (const other of sorted) {
      if (other !== run && other.lane >= 0 && overlaps(run, other)) taken.add(other.lane)
    }
    let lane = 0
    while (taken.has(lane)) lane++
    run.lane = lane
    count = Math.max(count, lane + 1)
  }
  return count
}

/** Free x-intervals of a row: between its nodes, and the open ends beyond the first and last. */
function rowGaps(row: BandRow, boxes: ReadonlyMap<string, Box>): [number, number][] {
  const sorted = row.ids.map((id) => boxes.get(id)).filter((box): box is Box => Boolean(box)).sort((a, b) => a.x - b.x)
  const gaps: [number, number][] = []
  let edge = -Infinity
  for (const box of sorted) {
    gaps.push([edge, box.x])
    edge = box.x + box.w
  }
  gaps.push([edge, Infinity])
  return gaps
}

function intersect(a: [number, number][], b: [number, number][]): [number, number][] {
  const out: [number, number][] = []
  for (const [a0, a1] of a) {
    for (const [b0, b1] of b) {
      const lo = Math.max(a0, b0)
      const hi = Math.min(a1, b1)
      if (hi - lo >= LANE_GAP * 2) out.push([lo, hi])
    }
  }
  return out
}

export function routeOrthogonal(
  edges: readonly OrthoEdge[],
  nodes: ReadonlyMap<string, OrthoNode>,
  rows: readonly BandRow[],
  /** Boxes labels must stay off, besides the nodes: the groups' label strips. */
  reserved: readonly Box[] = [],
): Map<string, RoutedPath> {
  const boxes = new Map<string, Box>()
  for (const [id, node] of nodes) boxes.set(id, node.box)
  const rowOf = new Map<string, number>()
  rows.forEach((row, index) => row.ids.forEach((id) => rowOf.set(id, index)))

  // Channels: one above the first row, one between every pair of rows, one below the last.
  const channels: Channel[] = []
  for (let index = 0; index <= rows.length; index++) {
    const above = rows[index - 1]
    const below = rows[index]
    if (above && below) {
      const top = above.y + above.h
      const gap = below.y - top
      channels.push({ centre: top + gap / 2, span: Math.max(gap - CHANNEL_INSET * 2, LANE_GAP), runs: [] })
    } else if (below) {
      channels.push({ centre: below.y - 36, span: 40, runs: [] })
    } else if (above) {
      channels.push({ centre: above.y + above.h + 36, span: 40, runs: [] })
    }
  }
  const leftEdge = Math.min(...[...boxes.values()].map((box) => box.x))
  const rightEdge = Math.max(...[...boxes.values()].map((box) => box.x + box.w))
  const corridors = new Map<string, Corridor>()
  const corridorFor = (kind: Corridor['kind'], x: number) => {
    const key = `${kind}:${Math.round(x)}`
    let corridor = corridors.get(key)
    if (!corridor) {
      corridor = { kind, x, runs: [] }
      corridors.set(key, corridor)
    }
    return corridor
  }

  // Pass 1: topology — which sides, channels and corridor every edge uses.
  const ports = new Map<string, { edgeId: string; node: string; side: Side; order: number }[]>()
  const requestPort = (edgeId: string, node: string, side: Side, order: number) => {
    const key = `${node}/${side}`
    const list = ports.get(key) ?? []
    list.push({ edgeId, node, side, order })
    ports.set(key, list)
  }

  const plans: Plan[] = []
  for (const edge of edges) {
    const s = boxes.get(edge.source)
    const t = boxes.get(edge.target)
    const sRow = rowOf.get(edge.source)
    const tRow = rowOf.get(edge.target)
    if (!s || !t || sRow === undefined || tRow === undefined) continue
    const sc = { x: s.x + s.w / 2, y: s.y + s.h / 2 }
    const tc = { x: t.x + t.w / 2, y: t.y + t.h / 2 }

    if (sRow === tRow) {
      // Neighbours in a row join side to side; anything further apart dips into the channel below.
      const between = rows[sRow].ids.some((id) => {
        const box = boxes.get(id)
        return box && id !== edge.source && id !== edge.target && box.x > Math.min(s.x, t.x) && box.x < Math.max(s.x, t.x)
      })
      if (!between) {
        const sourceSide: Side = tc.x > sc.x ? 'right' : 'left'
        const targetSide: Side = tc.x > sc.x ? 'left' : 'right'
        requestPort(edge.id, edge.source, sourceSide, tc.y)
        requestPort(edge.id, edge.target, targetSide, sc.y)
        plans.push({ edge, sourceSide, targetSide, channelA: -1, channelB: -1, corridor: null, corridorRun: null, runA: null, runB: null })
        continue
      }
      requestPort(edge.id, edge.source, 'bottom', tc.x)
      requestPort(edge.id, edge.target, 'bottom', sc.x)
      const run: Run = { edgeId: edge.id, from: sc.x, to: tc.x, lane: -1, pos: 0 }
      channels[sRow + 1].runs.push(run)
      plans.push({ edge, sourceSide: 'bottom', targetSide: 'bottom', channelA: sRow + 1, channelB: sRow + 1, corridor: null, corridorRun: null, runA: run, runB: null })
      continue
    }

    const down = tRow > sRow
    const sourceSide: Side = down ? 'bottom' : 'top'
    const targetSide: Side = down ? 'top' : 'bottom'
    const channelA = down ? sRow + 1 : sRow
    const channelB = down ? tRow : tRow + 1
    requestPort(edge.id, edge.source, sourceSide, tc.x)
    requestPort(edge.id, edge.target, targetSide, sc.x)

    if (channelA === channelB) {
      const run: Run = { edgeId: edge.id, from: sc.x, to: tc.x, lane: -1, pos: 0 }
      channels[channelA].runs.push(run)
      plans.push({ edge, sourceSide, targetSide, channelA, channelB, corridor: null, corridorRun: null, runA: run, runB: null })
      continue
    }

    // Rows between the two channels are obstacles; find a gap free in all of them.
    const first = Math.min(sRow, tRow) + 1
    const last = Math.max(sRow, tRow) - 1
    let free: [number, number][] = [[-Infinity, Infinity]]
    for (let index = first; index <= last; index++) free = intersect(free, rowGaps(rows[index], boxes))
    const ideal = (sc.x + tc.x) / 2
    let corridor: Corridor
    const bounded = free.filter(([lo, hi]) => Number.isFinite(lo) && Number.isFinite(hi))
    const containing = bounded.find(([lo, hi]) => ideal >= lo + LANE_GAP && ideal <= hi - LANE_GAP)
    if (containing) {
      corridor = corridorFor('gap', (containing[0] + containing[1]) / 2)
    } else if (bounded.length > 0) {
      const nearest = bounded.reduce((best, gap) =>
        Math.abs((gap[0] + gap[1]) / 2 - ideal) < Math.abs((best[0] + best[1]) / 2 - ideal) ? gap : best,
      )
      const margin = ideal < (leftEdge + rightEdge) / 2 ? leftEdge : rightEdge
      // A gap further away than the margin is not worth the detour.
      corridor =
        Math.abs((nearest[0] + nearest[1]) / 2 - ideal) <= Math.abs(margin - ideal)
          ? corridorFor('gap', (nearest[0] + nearest[1]) / 2)
          : corridorFor(ideal < (leftEdge + rightEdge) / 2 ? 'left' : 'right', margin)
    } else {
      corridor = corridorFor(ideal < (leftEdge + rightEdge) / 2 ? 'left' : 'right', ideal < (leftEdge + rightEdge) / 2 ? leftEdge : rightEdge)
    }
    const corridorRun: Run = { edgeId: edge.id, from: channels[channelA].centre, to: channels[channelB].centre, lane: -1, pos: 0 }
    corridor.runs.push(corridorRun)
    const runA: Run = { edgeId: edge.id, from: sc.x, to: corridor.x, lane: -1, pos: 0 }
    const runB: Run = { edgeId: edge.id, from: corridor.x, to: tc.x, lane: -1, pos: 0 }
    channels[channelA].runs.push(runA)
    channels[channelB].runs.push(runB)
    plans.push({ edge, sourceSide, targetSide, channelA, channelB, corridor, corridorRun, runA, runB })
  }

  // Pass 2: ports fanned per side, lanes per channel and corridor.
  const portPoint = new Map<string, Point>()
  for (const list of ports.values()) {
    list.sort((a, b) => a.order - b.order)
    const box = boxes.get(list[0].node)
    if (!box) continue
    const horizontal = list[0].side === 'top' || list[0].side === 'bottom'
    const length = horizontal ? box.w : box.h
    const usable = Math.max(length - PORT_INSET * 2, 0)
    list.forEach((request, index) => {
      const offset = PORT_INSET + (usable * (index + 1)) / (list.length + 1)
      const point: Point =
        request.side === 'top'
          ? { x: box.x + offset, y: box.y }
          : request.side === 'bottom'
            ? { x: box.x + offset, y: box.y + box.h }
            : request.side === 'left'
              ? { x: box.x, y: box.y + offset }
              : { x: box.x + box.w, y: box.y + offset }
      portPoint.set(`${request.edgeId}/${request.node}`, point)
    })
  }

  for (const channel of channels) {
    const count = assignLanes(channel.runs)
    const gap = count > 1 ? Math.min(LANE_GAP, channel.span / (count - 1)) : 0
    for (const run of channel.runs) run.pos = channel.centre + (run.lane - (count - 1) / 2) * gap
  }
  for (const corridor of corridors.values()) {
    const count = assignLanes(corridor.runs)
    for (const run of corridor.runs) {
      run.pos =
        corridor.kind === 'gap'
          ? corridor.x + (run.lane - (count - 1) / 2) * LANE_GAP
          : corridor.kind === 'left'
            ? corridor.x - MARGIN_CORRIDOR - run.lane * LANE_GAP
            : corridor.x + MARGIN_CORRIDOR + run.lane * LANE_GAP
    }
  }

  // Pass 3: points, then labels nudged apart.
  const routed = new Map<string, RoutedPath>()
  const labels: { id: string; box: Box }[] = []
  for (const plan of plans) {
    const { edge } = plan
    const start = portPoint.get(`${edge.id}/${edge.source}`)
    const end = portPoint.get(`${edge.id}/${edge.target}`)
    const source = nodes.get(edge.source)
    const target = nodes.get(edge.target)
    if (!start || !end || !source || !target) continue

    const points: Point[] = [start]
    let anchor: Point
    if (plan.channelA === -1) {
      // Side to side: a short jog when the two ports are not level.
      const midX = (start.x + end.x) / 2
      if (Math.abs(start.y - end.y) > 1) points.push({ x: midX, y: start.y }, { x: midX, y: end.y })
      // The gap between neighbours is narrower than most labels: the label sits above both nodes.
      anchor = { x: midX, y: Math.min(source.box.y, target.box.y) - LABEL_HEIGHT / 2 - 4 }
    } else if (plan.corridorRun && plan.runA && plan.runB) {
      const yA = plan.runA.pos
      const yB = plan.runB.pos
      const x = plan.corridorRun.pos
      points.push({ x: start.x, y: yA }, { x, y: yA }, { x, y: yB }, { x: end.x, y: yB })
      // The label goes on the longer of the two channel runs.
      const longA = Math.abs(start.x - x) >= Math.abs(end.x - x)
      anchor = longA ? { x: (start.x + x) / 2, y: yA } : { x: (end.x + x) / 2, y: yB }
    } else if (plan.runA) {
      const y = plan.runA.pos
      points.push({ x: start.x, y }, { x: end.x, y })
      anchor = { x: (start.x + end.x) / 2, y }
    } else {
      anchor = start
    }
    points.push(end)

    const route = routeThroughPoints(points, source, target, anchor)
    if (edge.labelWidth > 0) {
      // Try the anchor, then 14 px up/down steps, until the label sits on no other label and no node.
      const box: Box = { x: anchor.x - edge.labelWidth / 2, y: anchor.y - LABEL_HEIGHT / 2, w: edge.labelWidth, h: LABEL_HEIGHT }
      const clear = (candidate: Box) =>
        !labels.some((other) => boxesOverlap(candidate, other.box)) &&
        ![...boxes.values()].some((node) => boxesOverlap(candidate, node)) &&
        !reserved.some((area) => boxesOverlap(candidate, area))
      for (const step of [0, -1, 1, -2, 2, -3, 3]) {
        const candidate = { ...box, y: box.y + step * LABEL_NUDGE }
        if (clear(candidate)) {
          box.y = candidate.y
          break
        }
      }
      labels.push({ id: edge.id, box })
      route.mid = { x: box.x + box.w / 2, y: box.y + box.h / 2 }
    }
    routed.set(edge.id, route)
  }
  return routed
}
