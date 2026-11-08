import { BaseEdge, EdgeLabelRenderer, useStore, type EdgeProps, type ReactFlowState } from '@xyflow/react'
import { memo } from 'react'

import { cn } from '@/lib/utils'

import { RELATION_STYLE, relationLabel } from '../../lib/labels'
import type { Point } from './elk-layout'
import type { ArchEdge, WireState } from './types'

/** Corner radius of the orthogonal routes. */
const CORNER = 12
/** Labels of active wires only read above this zoom. */
const LABEL_ZOOM = 0.55
const labelsVisible = (state: ReactFlowState) => state.transform[2] >= LABEL_ZOOM

const fmt = (value: number) => (Math.round(value * 10) / 10).toString()

/**
 * ELK's route as an SVG path whose corners are quarter curves: each corner
 * is cut back along both runs by up to `radius` (never more than half a
 * run) and joined with a quadratic curve through the corner point.
 */
export function roundedPath(points: readonly Point[], radius = CORNER): string {
  if (points.length === 0) return ''
  let d = `M${fmt(points[0].x)},${fmt(points[0].y)}`
  for (let index = 1; index < points.length - 1; index++) {
    const a = points[index - 1]
    const b = points[index]
    const c = points[index + 1]
    const d1 = Math.hypot(b.x - a.x, b.y - a.y)
    const d2 = Math.hypot(c.x - b.x, c.y - b.y)
    if (d1 === 0 || d2 === 0) {
      d += ` L${fmt(b.x)},${fmt(b.y)}`
      continue
    }
    const r = Math.min(radius, d1 / 2, d2 / 2)
    const p1 = { x: b.x + ((a.x - b.x) * r) / d1, y: b.y + ((a.y - b.y) * r) / d1 }
    const p2 = { x: b.x + ((c.x - b.x) * r) / d2, y: b.y + ((c.y - b.y) * r) / d2 }
    d += ` L${fmt(p1.x)},${fmt(p1.y)} Q${fmt(b.x)},${fmt(b.y)} ${fmt(p2.x)},${fmt(p2.y)}`
  }
  const last = points[points.length - 1]
  return `${d} L${fmt(last.x)},${fmt(last.y)}`
}

/** Middle of the longest run: where a label has the most room. */
function labelPoint(points: readonly Point[]): Point {
  let best = { length: -1, x: points[0]?.x ?? 0, y: points[0]?.y ?? 0 }
  for (let index = 1; index < points.length; index++) {
    const a = points[index - 1]
    const b = points[index]
    const length = Math.hypot(b.x - a.x, b.y - a.y)
    if (length > best.length) best = { length, x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
  }
  return best
}

/** More folded relations, a slightly heavier line: 1px for one, up to 2.5px. */
function strokeWidth(count: number, state: WireState) {
  return 1 + Math.min(3, Math.log2(Math.max(1, count))) * 0.5 + (state === 'active' || state === 'linked' ? 0.5 : 0)
}

function opacity(state: WireState, expired: boolean) {
  if (state === 'dimmed') return 0.15
  if (expired) return 0.35
  return 1
}

/**
 * One wire: the trace in the relation's style. Touching the selection it is
 * drawn strong and labelled; while its card or the wire itself is hovered a
 * dash also moves along it, so the direction of the relation reads as flow.
 */
function WireEdgeView({ data }: EdgeProps<ArchEdge>) {
  const showLabel = useStore(labelsVisible)
  if (!data || data.points.length < 2) return null

  const path = roundedPath(data.points)
  const style = RELATION_STYLE[data.category]
  const active = data.state === 'active'
  const strong = active || data.state === 'linked'
  const label = `${data.relations.map(relationLabel).join(' · ')}${data.count > 1 ? ` ×${data.count}` : ''}`
  const at = labelPoint(data.points)

  return (
    <>
      <BaseEdge
        path={path}
        interactionWidth={14}
        style={{
          stroke: strong ? 'var(--foreground)' : style.accent ? 'var(--brand)' : 'var(--arch-edge)',
          strokeWidth: strokeWidth(data.count, data.state),
          strokeDasharray: strong ? undefined : style.dash,
          opacity: opacity(data.state, data.expired),
          transition: 'opacity var(--motion-fast) var(--motion-ease), stroke var(--motion-fast) var(--motion-ease)',
        }}
      />
      {active ? <path d={path} className="arch-flow-dash" /> : null}
      {strong && showLabel ? (
        <EdgeLabelRenderer>
          <div
            className={cn(
              'nodrag nopan pointer-events-none absolute rounded-sm bg-background px-1 py-px text-2xs leading-4 text-foreground ring-1 ring-border',
              data.expired && 'line-through',
            )}
            style={{ transform: `translate(-50%, -50%) translate(${at.x}px, ${at.y}px)` }}
          >
            {label}
          </div>
        </EdgeLabelRenderer>
      ) : null}
    </>
  )
}

export const WireEdge = memo(WireEdgeView)
