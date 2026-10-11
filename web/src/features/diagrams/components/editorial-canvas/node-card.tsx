import { createElement, type KeyboardEvent } from 'react'

import { NODE_KIND_LABEL, nodeIcon } from '../../lib/labels'
import type { LaidOutNode } from './layout'
import { fitText } from './text'

interface NodeCardProps {
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

const PAD_X = 12
const ICON = 16
const TEXT_X = PAD_X + ICON + 8

/** A 6 px-radius card: kind icon, 13 px label and an 11 px mono caption. Opens the code browser when it has a path. */
export function NodeCard({ item, emphasised, dimmed, onHover, onOpen, wasPan }: NodeCardProps) {
  const { node, box } = item
  const textWidth = box.w - TEXT_X - PAD_X
  const caption = node.caption?.trim() ?? ''
  const label = fitText(node.label, textWidth, 13)
  const hasCaption = caption.length > 0
  const interactive = Boolean(onOpen && node.path)

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
  const description = [kindLabel, caption, node.path ? `opens ${node.path}` : ''].filter(Boolean).join(', ')

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
      <rect
        width={box.w}
        height={box.h}
        rx={6}
        style={{
          fill: 'var(--card)',
          stroke: emphasised ? 'var(--foreground)' : 'var(--border)',
          strokeOpacity: emphasised ? 0.4 : 1,
          strokeWidth: 1,
        }}
      />
      <g style={{ color: 'var(--muted-foreground)' }}>
        {createElement(nodeIcon(node.kind), {
          x: PAD_X,
          y: (box.h - ICON) / 2,
          width: ICON,
          height: ICON,
          strokeWidth: 1.5,
          'aria-hidden': 'true',
        })}
      </g>
      <text
        x={TEXT_X}
        y={hasCaption ? 24 : box.h / 2 + 4.5}
        style={{ fill: 'var(--foreground)', fontSize: 13, fontWeight: 500 }}
      >
        {label}
      </text>
      {hasCaption ? (
        <text
          x={TEXT_X}
          y={41}
          style={{ fill: 'var(--muted-foreground)', fontSize: 11, fontFamily: 'var(--font-mono)' }}
        >
          {fitText(caption, textWidth, 11, true)}
        </text>
      ) : null}
      {interactive ? (
        <rect
          x={0.5}
          y={0.5}
          width={box.w - 1}
          height={box.h - 1}
          rx={6}
          className="pointer-events-none fill-none stroke-transparent group-focus-visible:stroke-ring"
          strokeWidth={2}
        />
      ) : null}
    </g>
  )
}
