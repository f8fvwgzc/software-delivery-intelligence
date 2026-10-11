import { EDGE_STYLE, EDGE_STYLES } from '../../lib/labels'

/** Open arrowheads, one per edge style plus the hover tint. `userSpaceOnUse` keeps them 8 px at any stroke width. */
export function Markers({ prefix }: { prefix: string }) {
  const entries = [
    ...EDGE_STYLES.map((style) => ({ id: style, color: EDGE_STYLE[style].color, opacity: EDGE_STYLE[style].opacity })),
    { id: 'hover', color: 'var(--brand)', opacity: 1 },
  ]
  return (
    <defs>
      {entries.map((entry) => (
        <marker
          key={entry.id}
          id={`${prefix}-${entry.id}`}
          markerWidth={10}
          markerHeight={10}
          refX={9}
          refY={5}
          orient="auto"
          markerUnits="userSpaceOnUse"
        >
          <path
            d="M 1 1 L 9 5 L 1 9"
            fill="none"
            style={{ stroke: entry.color, strokeOpacity: entry.opacity, strokeWidth: 1.2, strokeLinecap: 'round', strokeLinejoin: 'round' }}
          />
        </marker>
      ))}
    </defs>
  )
}
