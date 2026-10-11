/** Line ends the renderers share. `userSpaceOnUse` keeps them the same size at any stroke width. */
export type MarkerShape = 'open' | 'filled' | 'crow' | 'bar'

export type MarkerTone = 'line' | 'hover'

export const LINE_COLOR = 'var(--foreground)'
export const LINE_OPACITY = 0.6
export const HOVER_COLOR = 'var(--brand)'

const TONES: Record<MarkerTone, { color: string; opacity: number }> = {
  line: { color: LINE_COLOR, opacity: LINE_OPACITY },
  hover: { color: HOVER_COLOR, opacity: 1 },
}

export function markerUrl(prefix: string, shape: MarkerShape, tone: MarkerTone) {
  return `url(#${prefix}-${shape}-${tone})`
}

interface ShapeDef {
  size: number
  /** Where the line meets the marker; the tip of an arrowhead sits at `ref`. */
  ref: { x: number; y: number }
  path: string
  fill: boolean
}

const SHAPES: Record<MarkerShape, ShapeDef> = {
  // Open chevron, tip at the right.
  open: { size: 10, ref: { x: 9, y: 5 }, path: 'M 1 1 L 9 5 L 1 9', fill: false },
  // Filled triangle for sequence calls.
  filled: { size: 10, ref: { x: 9, y: 5 }, path: 'M 1 1.5 L 9 5 L 1 8.5 Z', fill: true },
  // Crow's foot (many) at the child end of a foreign key. As a start marker
  // (`auto-start-reverse`) +x points into the table, so the prongs spread at
  // the table's border and meet 12 px along the line.
  crow: { size: 14, ref: { x: 13, y: 7 }, path: 'M 1 7 L 13 1 M 1 7 L 13 7 M 1 7 L 13 13', fill: false },
  // Single bar (one) at the parent end.
  bar: { size: 14, ref: { x: 13, y: 7 }, path: 'M 9 1 V 13 M 9 7 H 13', fill: false },
}

const SHAPE_IDS = Object.keys(SHAPES) as MarkerShape[]
const TONE_IDS = Object.keys(TONES) as MarkerTone[]

export function Markers({ prefix }: { prefix: string }) {
  return (
    <defs>
      {SHAPE_IDS.flatMap((shape) =>
        TONE_IDS.map((tone) => {
          const def = SHAPES[shape]
          const paint = TONES[tone]
          return (
            <marker
              key={`${shape}-${tone}`}
              id={`${prefix}-${shape}-${tone}`}
              markerWidth={def.size}
              markerHeight={def.size}
              refX={def.ref.x}
              refY={def.ref.y}
              orient="auto-start-reverse"
              markerUnits="userSpaceOnUse"
            >
              <path
                d={def.path}
                style={{
                  fill: def.fill ? paint.color : 'none',
                  fillOpacity: def.fill ? paint.opacity : undefined,
                  stroke: paint.color,
                  strokeOpacity: paint.opacity,
                  strokeWidth: 1.2,
                  strokeLinecap: 'round',
                  strokeLinejoin: 'round',
                }}
              />
            </marker>
          )
        }),
      )}
    </defs>
  )
}
