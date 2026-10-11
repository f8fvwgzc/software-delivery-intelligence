import type { CSSProperties } from 'react'

import type { NodeShape } from '../../../lib/labels'

interface ShapeOutlineProps {
  shape: NodeShape
  w: number
  h: number
  style?: CSSProperties
  className?: string
}

/** Depth of the cylinder's top ellipse. */
export function cylinderRy(h: number) {
  return Math.min(8, h / 6)
}

/** How far the hexagon's points stick out past its flat top and bottom. */
export function hexagonIndent(w: number) {
  return Math.min(14, w / 5)
}

/** Horizontal room a shape's slanted or rounded sides take away from the text. */
export function shapeInset(shape: NodeShape, h: number): number {
  switch (shape) {
    case 'stadium':
      return Math.round(h * 0.3)
    case 'hexagon':
      return 12
    case 'subroutine':
      return 6
    default:
      return 0
  }
}

/** Vertical room the cylinder's top ellipse takes away from the text. */
export function shapeTopInset(shape: NodeShape, h: number): number {
  return shape === 'cylinder' ? cylinderRy(h) : 0
}

function cylinderPath(w: number, h: number) {
  const ry = cylinderRy(h)
  const rx = w / 2
  return `M 0 ${ry} A ${rx} ${ry} 0 0 1 ${w} ${ry} V ${h - ry} A ${rx} ${ry} 0 0 1 0 ${h - ry} Z`
}

function hexagonPoints(w: number, h: number) {
  const indent = hexagonIndent(w)
  return `${indent},0 ${w - indent},0 ${w},${h / 2} ${w - indent},${h} ${indent},${h} 0,${h / 2}`
}

/**
 * The outline of a node kind in a w×h box at the origin: fill, stroke and
 * their opacities come from `style`. Every element carries the same
 * style, so a focus ring can reuse the component with a ring stroke.
 */
export function ShapeOutline({ shape, w, h, style, className }: ShapeOutlineProps) {
  switch (shape) {
    case 'circle': {
      const r = Math.min(w, h) / 2
      return <circle cx={w / 2} cy={h / 2} r={r} style={style} className={className} />
    }
    case 'rounded':
      return <rect width={w} height={h} rx={12} style={style} className={className} />
    case 'stadium':
      return <rect width={w} height={h} rx={h / 2} style={style} className={className} />
    case 'cylinder': {
      const ry = cylinderRy(h)
      return (
        <g className={className}>
          <path d={cylinderPath(w, h)} style={style} />
          {/* The front half of the top ellipse, stroke only, so the lid reads as a rim. */}
          <path d={`M 0 ${ry} A ${w / 2} ${ry} 0 0 0 ${w} ${ry}`} style={{ ...style, fill: 'none' }} />
        </g>
      )
    }
    case 'hexagon':
      return <polygon points={hexagonPoints(w, h)} style={{ strokeLinejoin: 'round', ...style }} className={className} />
    case 'subroutine':
      return (
        <g className={className}>
          <rect width={w} height={h} rx={2} style={style} />
          <path d={`M 5 0 V ${h} M ${w - 5} 0 V ${h}`} style={{ ...style, fill: 'none' }} />
        </g>
      )
    default:
      return <rect width={w} height={h} rx={6} style={style} className={className} />
  }
}
