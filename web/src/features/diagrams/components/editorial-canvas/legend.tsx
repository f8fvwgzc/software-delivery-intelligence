import type { ReactNode } from 'react'

import { textWidth } from './text'
import type { Box } from './types'

/** One legend entry: a 32×16 sample drawn at the origin, then its wording. */
export interface LegendItem {
  id: string
  label: string
  sample: ReactNode
}

export const SAMPLE_WIDTH = 32
export const SAMPLE_HEIGHT = 16
const ROW_HEIGHT = 24
const RULE_GAP = 12
const BOTTOM_PAD = 4
const ITEM_GAP = 24
const LABEL_GAP = 8
const LABEL_FONT = { size: 11 }

export interface LegendRow {
  items: { item: LegendItem; x: number }[]
}

export interface LegendLayout {
  rows: LegendRow[]
  /** Space the strip takes under the drawing, 0 when there is nothing to explain. */
  height: number
}

function itemWidth(item: LegendItem) {
  return SAMPLE_WIDTH + LABEL_GAP + textWidth(item.label, LABEL_FONT)
}

/** Flows the items into rows no wider than `width`; the strip height follows. */
export function layoutLegend(items: readonly LegendItem[], width: number): LegendLayout {
  if (items.length === 0) return { rows: [], height: 0 }
  const rows: LegendRow[] = [{ items: [] }]
  let x = 0
  for (const item of items) {
    const w = itemWidth(item)
    const row = rows[rows.length - 1]
    if (row.items.length > 0 && x + w > width) {
      rows.push({ items: [{ item, x: 0 }] })
      x = w + ITEM_GAP
      continue
    }
    row.items.push({ item, x })
    x += w + ITEM_GAP
  }
  return { rows, height: RULE_GAP + rows.length * ROW_HEIGHT + BOTTOM_PAD }
}

interface LegendProps {
  layout: LegendLayout
  /** The whole drawing; the strip sits inside its bottom `layout.height`, inset by `inset`. */
  bounds: Box
  inset: number
}

/** A strip along the bottom of the drawing: a hairline, then the samples with their wording. */
export function Legend({ layout, bounds, inset }: LegendProps) {
  if (layout.rows.length === 0) return null
  const top = bounds.y + bounds.h - layout.height
  const left = bounds.x + inset
  return (
    <g aria-hidden="true" style={{ fontSize: LABEL_FONT.size }}>
      <line
        x1={left}
        x2={bounds.x + bounds.w - inset}
        y1={top + 0.5}
        y2={top + 0.5}
        style={{ stroke: 'var(--border)', strokeWidth: 1 }}
      />
      {layout.rows.map((row, rowIndex) => {
        const y = top + RULE_GAP + rowIndex * ROW_HEIGHT + (ROW_HEIGHT - SAMPLE_HEIGHT) / 2
        return row.items.map(({ item, x }) => (
          <g key={item.id} transform={`translate(${left + x} ${y})`}>
            {item.sample}
            <text
              x={SAMPLE_WIDTH + LABEL_GAP}
              y={SAMPLE_HEIGHT / 2}
              dominantBaseline="central"
              style={{ fill: 'var(--muted-foreground)' }}
            >
              {item.label}
            </text>
          </g>
        ))
      })}
    </g>
  )
}
