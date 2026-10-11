import { HOVER_COLOR, LINE_COLOR, LINE_OPACITY, markerUrl, type MarkerShape } from '../markers'
import { EDGE_LABEL_FONT, type LaidOutEdge } from './layout'

interface EdgePathProps {
  item: LaidOutEdge
  markerPrefix: string
  emphasised: boolean
  dimmed: boolean
  /** Line ends; an open arrowhead at the target by default. */
  ends?: { start?: MarkerShape; end: MarkerShape }
}

const ARROW: { start?: MarkerShape; end: MarkerShape } = { end: 'open' }

export const DASH = '5 4'

/** One routed edge: a foreground/60 line with an open arrowhead, dashed when inferred, its label haloed on the line. */
export function EdgePath({ item, markerPrefix, emphasised, dimmed, ends = ARROW }: EdgePathProps) {
  const tone = emphasised ? 'hover' : 'line'
  return (
    <g
      data-dim
      style={{ opacity: dimmed ? 0.18 : 1 }}
      className="transition-opacity duration-150 motion-reduce:transition-none"
    >
      <path
        d={item.route.d}
        fill="none"
        markerStart={ends.start ? markerUrl(markerPrefix, ends.start, tone) : undefined}
        markerEnd={markerUrl(markerPrefix, ends.end, tone)}
        style={{
          stroke: emphasised ? HOVER_COLOR : LINE_COLOR,
          strokeOpacity: emphasised ? 1 : LINE_OPACITY,
          strokeWidth: emphasised ? 1.5 : 1.2,
          strokeDasharray: item.dashed ? DASH : undefined,
          strokeLinecap: 'round',
          strokeLinejoin: 'round',
        }}
      />
      {item.label ? (
        <text
          x={item.route.mid.x}
          y={item.route.mid.y}
          textAnchor="middle"
          dominantBaseline="central"
          style={{
            fill: emphasised ? 'var(--foreground)' : 'var(--muted-foreground)',
            fontSize: EDGE_LABEL_FONT.size,
            paintOrder: 'stroke',
            stroke: 'var(--background)',
            strokeWidth: 4,
            strokeLinejoin: 'round',
          }}
        >
          {item.label}
          {item.edge.evidencePath ? <title>{`Evidence: ${item.edge.evidencePath}`}</title> : null}
        </text>
      ) : null}
    </g>
  )
}
