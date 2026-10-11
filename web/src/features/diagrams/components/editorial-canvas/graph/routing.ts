import type { NodeShape } from '../../../lib/labels'
import type { Box, Point } from '../types'

export interface RoutedPath {
  /** The bends, after trimming to the end shapes and dropping collinear points. */
  points: Point[]
  /** SVG path: straight runs with rounded corners. */
  d: string
  /** Where the label sits: dagre's label position, else the middle of the polyline. */
  mid: Point
}

export interface EndShape {
  box: Box
  shape: NodeShape
}

const CORNER_RADIUS = 12

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

/** Drops repeated and collinear points so every remaining one is a real bend. */
function simplify(points: readonly Point[]): Point[] {
  const out: Point[] = []
  for (const point of points) {
    const last = out[out.length - 1]
    if (last && distance(last, point) < 0.5) continue
    out.push(point)
  }
  for (let index = 1; index < out.length - 1; ) {
    const a = out[index - 1]
    const b = out[index]
    const c = out[index + 1]
    // Cross product of ab × bc: zero when b lies on the line through a and c.
    const cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x)
    if (Math.abs(cross) < 0.5) out.splice(index, 1)
    else index++
  }
  return out
}

/**
 * Dagre ends every edge on the node's rectangle. A circle is smaller than
 * its box, so its end point is moved onto the circle along the last run.
 */
function trimToShape(end: Point, neighbour: Point, target: EndShape): Point {
  if (target.shape !== 'circle') return end
  const { box } = target
  const centre = { x: box.x + box.w / 2, y: box.y + box.h / 2 }
  const r = Math.min(box.w, box.h) / 2
  return towards(centre, neighbour, r)
}

/** Polyline → path whose bends are quadratic arcs of up to `CORNER_RADIUS`. */
function roundedPath(points: readonly Point[]): string {
  if (points.length === 0) return ''
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

/** The point halfway along the polyline's length. */
function midpoint(points: readonly Point[]): Point {
  let total = 0
  for (let index = 1; index < points.length; index++) total += distance(points[index - 1], points[index])
  let remaining = total / 2
  for (let index = 1; index < points.length; index++) {
    const run = distance(points[index - 1], points[index])
    if (remaining <= run) return towards(points[index - 1], points[index], remaining)
    remaining -= run
  }
  return points[0] ?? { x: 0, y: 0 }
}

/**
 * Turns dagre's edge points (which already bend around ranks, clusters
 * and the edge's own label) into a drawable path: the ends are moved onto
 * the end shapes, the bends rounded.
 */
export function routeThroughPoints(
  raw: readonly Point[],
  source: EndShape,
  target: EndShape,
  label: Point | null,
): RoutedPath {
  const points = simplify(raw)
  if (points.length >= 2) {
    points[0] = trimToShape(points[0], points[1], source)
    const last = points.length - 1
    points[last] = trimToShape(points[last], points[last - 1], target)
  }
  return { points, d: roundedPath(points), mid: label ?? midpoint(points) }
}
