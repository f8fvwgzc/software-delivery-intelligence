import type { Box, Point } from './types'

export type Side = 'left' | 'right' | 'top' | 'bottom'

export interface EdgeEndpoints {
  id: string
  source: string
  target: string
}

export interface RoutedPath {
  /** Orthogonal polyline before corner rounding. */
  points: Point[]
  /** SVG path with quarter-arc corners. */
  d: string
  /** Where the label pill sits: the middle of the longest run. */
  mid: Point
}

const CORNER_RADIUS = 8
/** Ports stay at least this far from a corner, so they never land on the radius. */
const PORT_INSET = 12

interface PortRequest {
  edgeId: string
  node: string
  side: Side
  /** Coordinate of the other endpoint along the edge, used to order ports. */
  order: number
}

function sidesFor(direction: 'LR' | 'TB', s: Box, t: Box): [Side, Side] {
  if (direction === 'LR') {
    if (t.x >= s.x + s.w) return ['right', 'left']
    if (t.x + t.w <= s.x) return ['left', 'right']
    return t.y > s.y ? ['bottom', 'top'] : ['top', 'bottom']
  }
  if (t.y >= s.y + s.h) return ['bottom', 'top']
  if (t.y + t.h <= s.y) return ['top', 'bottom']
  return t.x > s.x ? ['right', 'left'] : ['left', 'right']
}

function isHorizontal(side: Side) {
  return side === 'left' || side === 'right'
}

function portPoint(box: Box, side: Side, offset: number): Point {
  switch (side) {
    case 'left':
      return { x: box.x, y: box.y + offset }
    case 'right':
      return { x: box.x + box.w, y: box.y + offset }
    case 'top':
      return { x: box.x + offset, y: box.y }
    case 'bottom':
      return { x: box.x + offset, y: box.y + box.h }
  }
}

/**
 * Fans the ports of every (node, side) pair: N connectors on an edge of
 * length L sit at L * k / (N + 1), ordered by where their other end is, so
 * no two connectors share a point and parallel ones never cross.
 */
interface Port extends Point {
  /** Signed distance of this port from the middle of its side, in lanes of `LANE_GAP`; parallel runs are spread by it. */
  lane: number
}

/** Parallel runs that leave the same side are spread this far apart, so none hides another. */
const LANE_GAP = 12

function assignPorts(requests: PortRequest[], boxes: ReadonlyMap<string, Box>): Map<string, Port> {
  const bySide = new Map<string, PortRequest[]>()
  for (const request of requests) {
    const key = `${request.node}/${request.side}`
    const list = bySide.get(key)
    if (list) list.push(request)
    else bySide.set(key, [request])
  }

  const ports = new Map<string, Port>()
  for (const list of bySide.values()) {
    list.sort((a, b) => a.order - b.order)
    const box = boxes.get(list[0].node)
    if (!box) continue
    const length = isHorizontal(list[0].side) ? box.h : box.w
    const usable = Math.max(length - PORT_INSET * 2, 0)
    list.forEach((request, index) => {
      const offset = PORT_INSET + (usable * (index + 1)) / (list.length + 1)
      const lane = index - (list.length - 1) / 2
      ports.set(`${request.edgeId}/${request.node}`, { ...portPoint(box, request.side, offset), lane })
    })
  }
  return ports
}

/**
 * Turns a free polyline into one made only of horizontal and vertical runs.
 * `shift` moves every cross run off the exact middle, so edges that leave
 * the same side through different ports do not share a vertical.
 */
function orthogonalise(points: Point[], horizontalFirst: boolean, shift: number): Point[] {
  const out: Point[] = [points[0]]
  for (let index = 1; index < points.length; index++) {
    const a = out[out.length - 1]
    const b = points[index]
    if (horizontalFirst) {
      if (Math.abs(a.y - b.y) < 1) {
        out.push({ x: b.x, y: a.y })
      } else {
        const midX = (a.x + b.x) / 2 + shift
        out.push({ x: midX, y: a.y }, { x: midX, y: b.y }, { x: b.x, y: b.y })
      }
    } else if (Math.abs(a.x - b.x) < 1) {
      out.push({ x: a.x, y: b.y })
    } else {
      const midY = (a.y + b.y) / 2 + shift
      out.push({ x: a.x, y: midY }, { x: b.x, y: midY }, { x: b.x, y: b.y })
    }
  }
  return out
}

/** Drops repeated and collinear points so every remaining one is a real bend. */
function simplify(points: Point[]): Point[] {
  const out: Point[] = []
  for (const point of points) {
    const last = out[out.length - 1]
    if (last && Math.abs(last.x - point.x) < 0.5 && Math.abs(last.y - point.y) < 0.5) continue
    out.push(point)
  }
  for (let index = 1; index < out.length - 1; ) {
    const a = out[index - 1]
    const b = out[index]
    const c = out[index + 1]
    const sameX = Math.abs(a.x - b.x) < 0.5 && Math.abs(b.x - c.x) < 0.5
    const sameY = Math.abs(a.y - b.y) < 0.5 && Math.abs(b.y - c.y) < 0.5
    if (sameX || sameY) out.splice(index, 1)
    else index++
  }
  return out
}

function distance(a: Point, b: Point) {
  return Math.hypot(b.x - a.x, b.y - a.y)
}

function towards(from: Point, to: Point, length: number): Point {
  const total = distance(from, to)
  if (total === 0) return from
  const ratio = length / total
  return { x: from.x + (to.x - from.x) * ratio, y: from.y + (to.y - from.y) * ratio }
}

const fmt = (value: number) => (Math.round(value * 100) / 100).toString()

/** Polyline → path string whose bends are quarter arcs of up to `CORNER_RADIUS`. */
function roundedPath(points: Point[]): string {
  if (points.length === 0) return ''
  if (points.length === 1) return `M ${fmt(points[0].x)} ${fmt(points[0].y)}`
  let d = `M ${fmt(points[0].x)} ${fmt(points[0].y)}`
  for (let index = 1; index < points.length - 1; index++) {
    const prev = points[index - 1]
    const corner = points[index]
    const next = points[index + 1]
    const radius = Math.min(CORNER_RADIUS, distance(prev, corner) / 2, distance(corner, next) / 2)
    const entry = towards(corner, prev, radius)
    const exit = towards(corner, next, radius)
    d += ` L ${fmt(entry.x)} ${fmt(entry.y)} Q ${fmt(corner.x)} ${fmt(corner.y)} ${fmt(exit.x)} ${fmt(exit.y)}`
  }
  const last = points[points.length - 1]
  d += ` L ${fmt(last.x)} ${fmt(last.y)}`
  return d
}

/** The middle of the longest run: the one place a label pill is sure to sit on open canvas. */
function labelAnchor(points: Point[]): Point {
  let best = 0
  let bestLength = -1
  for (let index = 1; index < points.length; index++) {
    const length = distance(points[index - 1], points[index])
    if (length > bestLength) {
      bestLength = length
      best = index
    }
  }
  if (best === 0) return points[0] ?? { x: 0, y: 0 }
  return towards(points[best - 1], points[best], bestLength / 2)
}

/**
 * Routes every edge between node boxes: ports fanned per side, one
 * lane-shifted elbow between them, corners rounded.
 */
export function routeEdges(
  edges: readonly EdgeEndpoints[],
  boxes: ReadonlyMap<string, Box>,
  direction: 'LR' | 'TB',
): Map<string, RoutedPath> {
  const sides = new Map<string, [Side, Side]>()
  const requests: PortRequest[] = []

  for (const edge of edges) {
    const s = boxes.get(edge.source)
    const t = boxes.get(edge.target)
    if (!s || !t) continue
    const [sourceSide, targetSide] = sidesFor(direction, s, t)
    sides.set(edge.id, [sourceSide, targetSide])
    const sourceCentre = { x: s.x + s.w / 2, y: s.y + s.h / 2 }
    const targetCentre = { x: t.x + t.w / 2, y: t.y + t.h / 2 }
    requests.push({
      edgeId: edge.id,
      node: edge.source,
      side: sourceSide,
      order: isHorizontal(sourceSide) ? targetCentre.y : targetCentre.x,
    })
    requests.push({
      edgeId: edge.id,
      node: edge.target,
      side: targetSide,
      order: isHorizontal(targetSide) ? sourceCentre.y : sourceCentre.x,
    })
  }

  const ports = assignPorts(requests, boxes)
  const routed = new Map<string, RoutedPath>()

  for (const edge of edges) {
    const pair = sides.get(edge.id)
    const start = ports.get(`${edge.id}/${edge.source}`)
    const end = ports.get(`${edge.id}/${edge.target}`)
    if (!pair || !start || !end) continue

    const [sourceSide] = pair
    const horizontalFirst = isHorizontal(sourceSide)
    const points = simplify(orthogonalise([start, end], horizontalFirst, start.lane * LANE_GAP))
    routed.set(edge.id, { points, d: roundedPath(points), mid: labelAnchor(points) })
  }

  return routed
}
