import type { KeyboardEvent } from 'react'

import { NODE_KIND_LABEL } from '../../../lib/labels'
import { FILL_ALPHA, STROKE_ALPHA } from '../palette'
import { CIRCLE_FONT, LABEL_FONT, SUB_FONT, type LaidOutNode } from './layout'
import { ShapeOutline } from './node-shape'

interface GraphNodeProps {
  item: LaidOutNode
  /** Hovered or adjacent to the hovered node. */
  emphasised: boolean
  /** Something else is hovered and this node is not connected to it. */
  dimmed: boolean
  onHover: (id: string | null) => void
  onOpen?: (path: string) => void
  /** Reports whether the pointer gesture that just ended was a pan. */
  wasPan: () => boolean
}

const LABEL_LINE = 16
const SUB_LINE = 14

/**
 * A node drawn as its kind's shape in its group's tint: the label centred
 * (two lines at most), the file name or caption in mono under it. Opens the
 * code browser when it has a path.
 */
export function GraphNode({ item, emphasised, dimmed, onHover, onOpen, wasPan }: GraphNodeProps) {
  const { node, box, shape, tint, lines, sub, inset } = item
  const interactive = Boolean(onOpen && node.path)
  const circle = shape === 'circle'
  const labelFont = circle ? CIRCLE_FONT : LABEL_FONT
  const lineHeight = circle ? 13 : LABEL_LINE
  const textHeight = lines.length * lineHeight + (sub ? SUB_LINE : 0)
  const top = inset.top + (box.h - inset.top - textHeight) / 2

  const open = () => {
    if (interactive && node.path && !wasPan()) onOpen?.(node.path)
  }
  const onKeyDown = (event: KeyboardEvent<SVGGElement>) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      if (interactive && node.path) onOpen?.(node.path)
    }
  }

  const kindLabel = NODE_KIND_LABEL[node.kind] ?? node.kind
  const description = [kindLabel, node.caption?.trim() ?? '', node.path ? `opens ${node.path}` : ''].filter(Boolean).join(', ')

  return (
    <g
      transform={`translate(${box.x} ${box.y})`}
      data-dim
      style={{ opacity: dimmed ? 0.35 : 1, cursor: interactive ? 'pointer' : 'default' }}
      className="group outline-none transition-opacity duration-150 motion-reduce:transition-none"
      role={interactive ? 'link' : undefined}
      tabIndex={interactive ? 0 : undefined}
      aria-label={`${node.label}: ${description}`}
      onPointerEnter={() => onHover(node.id)}
      onPointerLeave={() => onHover(null)}
      onFocus={() => onHover(node.id)}
      onBlur={() => onHover(null)}
      onClick={open}
      onKeyDown={onKeyDown}
    >
      {node.description ? <title>{node.description}</title> : null}
      <ShapeOutline
        shape={shape}
        w={box.w}
        h={box.h}
        style={{
          fill: tint.hue,
          fillOpacity: FILL_ALPHA,
          stroke: tint.hue,
          strokeOpacity: emphasised ? 1 : STROKE_ALPHA,
          strokeWidth: emphasised ? 1.5 : 1,
        }}
      />
      <text
        x={box.w / 2}
        textAnchor="middle"
        style={{ fill: tint.fg, fontSize: labelFont.size, fontWeight: labelFont.weight }}
      >
        {lines.map((line, index) => (
          <tspan key={index} x={box.w / 2} y={top + index * lineHeight + lineHeight / 2} dominantBaseline="central">
            {line}
          </tspan>
        ))}
      </text>
      {sub ? (
        <text
          x={box.w / 2}
          y={top + lines.length * lineHeight + SUB_LINE / 2}
          textAnchor="middle"
          dominantBaseline="central"
          style={{ fill: tint.fg, fillOpacity: 0.75, fontSize: SUB_FONT.size, fontFamily: 'var(--font-mono)' }}
        >
          {sub}
        </text>
      ) : null}
      {interactive ? (
        <ShapeOutline
          shape={shape}
          w={box.w}
          h={box.h}
          className="pointer-events-none fill-none stroke-transparent [stroke-width:2] group-focus-visible:stroke-ring"
        />
      ) : null}
    </g>
  )
}
