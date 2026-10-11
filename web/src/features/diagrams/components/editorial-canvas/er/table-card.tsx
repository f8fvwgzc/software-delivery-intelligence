import type { KeyboardEvent } from 'react'

import { FILL_ALPHA, STROKE_ALPHA, type Tint } from '../palette'
import { textWidth } from '../text'
import {
  BADGE_GAP,
  BADGE_WIDTH,
  CARD_PAD_X,
  COLUMN_FONT,
  HEADER_HEIGHT,
  NOTE_FONT,
  ROW_HEIGHT,
  TABLE_NAME_FONT,
  TYPE_FONT,
  type LaidOutTable,
} from './layout'

interface TableCardProps {
  item: LaidOutTable
  emphasised: boolean
  dimmed: boolean
  onHover: (id: string | null) => void
  onOpen?: (path: string) => void
  wasPan: () => boolean
}

const BADGE_HEIGHT = 12

/** A tiny "PK" / "FK" tag after a column name. */
export function KeyBadge({ kind, x, y, tint }: { kind: 'PK' | 'FK'; x: number; y: number; tint: Tint }) {
  return (
    <g transform={`translate(${x} ${y - BADGE_HEIGHT / 2})`}>
      <rect
        width={BADGE_WIDTH}
        height={BADGE_HEIGHT}
        rx={3}
        style={{ fill: tint.hue, fillOpacity: FILL_ALPHA, stroke: tint.hue, strokeOpacity: STROKE_ALPHA, strokeWidth: 1 }}
      />
      <text
        x={BADGE_WIDTH / 2}
        y={BADGE_HEIGHT / 2}
        textAnchor="middle"
        dominantBaseline="central"
        style={{ fill: tint.fg, fontSize: 8, fontWeight: 600, letterSpacing: '0.04em' }}
      >
        {kind}
      </text>
    </g>
  )
}

/** A table: tinted header with the name in mono, then a row per column with its key badges and type. */
export function TableCard({ item, emphasised, dimmed, onHover, onOpen, wasPan }: TableCardProps) {
  const { node, box, tint, name, columns, note } = item
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

  const summary = columns.length > 0 ? `${columns.length} columns shown` : note
  const description = ['Table', summary, node.path ? `opens ${node.path}` : ''].filter(Boolean).join(', ')

  return (
    <g
      transform={`translate(${box.x} ${box.y})`}
      data-dim
      style={{ opacity: dimmed ? 0.35 : 1, cursor: interactive ? 'pointer' : 'default' }}
      className="group outline-none transition-opacity duration-150 motion-reduce:transition-none"
      role={interactive ? 'link' : undefined}
      tabIndex={interactive ? 0 : undefined}
      aria-label={`${name}: ${description}`}
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
          stroke: tint.hue,
          strokeOpacity: emphasised ? 1 : STROKE_ALPHA,
          strokeWidth: emphasised ? 1.5 : 1,
        }}
      />
      {/* Header: the top corners follow the card, the bottom edge is square. */}
      <path
        d={`M 0 ${HEADER_HEIGHT} V 6 Q 0 0 6 0 H ${box.w - 6} Q ${box.w} 0 ${box.w} 6 V ${HEADER_HEIGHT} Z`}
        style={{ fill: tint.hue, fillOpacity: FILL_ALPHA }}
      />
      <line
        x1={0}
        x2={box.w}
        y1={HEADER_HEIGHT + 0.5}
        y2={HEADER_HEIGHT + 0.5}
        style={{ stroke: tint.hue, strokeOpacity: STROKE_ALPHA, strokeWidth: 1 }}
      />
      <text
        x={box.w / 2}
        y={HEADER_HEIGHT / 2}
        textAnchor="middle"
        dominantBaseline="central"
        style={{ fill: tint.fg, fontSize: TABLE_NAME_FONT.size, fontWeight: TABLE_NAME_FONT.weight, fontFamily: 'var(--font-mono)' }}
      >
        {name}
      </text>
      {columns.map((column, index) => {
        const y = HEADER_HEIGHT + index * ROW_HEIGHT + ROW_HEIGHT / 2
        let badgeX = CARD_PAD_X + textWidth(column.shownName, COLUMN_FONT) + BADGE_GAP
        const badges: ('PK' | 'FK')[] = []
        if (column.pk) badges.push('PK')
        if (column.fk !== null) badges.push('FK')
        return (
          <g key={`${column.name}-${index}`}>
            {index > 0 ? (
              <line
                x1={CARD_PAD_X / 2}
                x2={box.w - CARD_PAD_X / 2}
                y1={y - ROW_HEIGHT / 2 + 0.5}
                y2={y - ROW_HEIGHT / 2 + 0.5}
                style={{ stroke: 'var(--border)', strokeWidth: 1 }}
              />
            ) : null}
            <text
              x={CARD_PAD_X}
              y={y}
              dominantBaseline="central"
              style={{ fill: 'var(--foreground)', fontSize: COLUMN_FONT.size, fontFamily: 'var(--font-mono)' }}
            >
              {column.shownName}
              {column.fk ? <title>{`→ ${column.fk}`}</title> : null}
            </text>
            {badges.map((kind) => {
              const x = badgeX
              badgeX += BADGE_WIDTH + BADGE_GAP
              return <KeyBadge key={kind} kind={kind} x={x} y={y} tint={tint} />
            })}
            {column.shownType ? (
              <text
                x={box.w - CARD_PAD_X}
                y={y}
                textAnchor="end"
                dominantBaseline="central"
                style={{ fill: 'var(--muted-foreground)', fontSize: TYPE_FONT.size }}
              >
                {column.shownType}
                {column.notNull || column.unique ? (
                  <title>{[column.notNull ? 'NOT NULL' : '', column.unique ? 'UNIQUE' : ''].filter(Boolean).join(', ')}</title>
                ) : null}
              </text>
            ) : null}
          </g>
        )
      })}
      {note ? (
        <text
          x={box.w / 2}
          y={HEADER_HEIGHT + columns.length * ROW_HEIGHT + ROW_HEIGHT / 2}
          textAnchor="middle"
          dominantBaseline="central"
          style={{ fill: 'var(--muted-foreground)', fontSize: NOTE_FONT.size, fontStyle: 'italic' }}
        >
          {note}
        </text>
      ) : null}
      {interactive ? (
        <rect
          width={box.w}
          height={box.h}
          rx={6}
          className="pointer-events-none fill-none stroke-transparent [stroke-width:2] group-focus-visible:stroke-ring"
        />
      ) : null}
    </g>
  )
}
