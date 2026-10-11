import { EDGE_STYLE, type EdgeStyle } from '../../lib/labels'
import { LEGEND_HEIGHT } from './layout'
import { textWidth } from './text'
import type { Box } from './types'

interface LegendProps {
  styles: readonly EdgeStyle[]
  bounds: Box
  markerPrefix: string
}

const SAMPLE = 28
const GAP = 24

/** Left edge of every item, laid out in a row. */
function itemOffsets(styles: readonly EdgeStyle[], start: number): number[] {
  const offsets: number[] = []
  let x = start
  for (const style of styles) {
    offsets.push(x)
    x += SAMPLE + 8 + textWidth(EDGE_STYLE[style].label, 11) + GAP
  }
  return offsets
}

/** A strip along the bottom of the drawing, only rendered when more than one edge style is in use. */
export function Legend({ styles, bounds, markerPrefix }: LegendProps) {
  const top = bounds.y + bounds.h - LEGEND_HEIGHT
  const y = top + LEGEND_HEIGHT / 2 + 4
  const offsets = itemOffsets(styles, bounds.x + 24)

  return (
    <g aria-hidden="true">
      <line
        x1={bounds.x + 24}
        x2={bounds.x + bounds.w - 24}
        y1={top + 4}
        y2={top + 4}
        style={{ stroke: 'var(--border)', strokeWidth: 1 }}
      />
      {styles.map((style, index) => {
        const meta = EDGE_STYLE[style]
        return (
          <g key={style} transform={`translate(${offsets[index]} ${y})`}>
            <path
              d={`M 0 -4 H ${SAMPLE}`}
              fill="none"
              markerEnd={`url(#${markerPrefix}-${style})`}
              style={{
                stroke: meta.color,
                strokeOpacity: meta.opacity,
                strokeWidth: 1.2,
                strokeDasharray: meta.dashed ? '5 4' : undefined,
              }}
            />
            <text x={SAMPLE + 8} style={{ fill: 'var(--muted-foreground)', fontSize: 11 }}>
              {meta.label}
            </text>
          </g>
        )
      })}
    </g>
  )
}
