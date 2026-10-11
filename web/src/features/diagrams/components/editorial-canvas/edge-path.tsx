import { EDGE_STYLE } from '../../lib/labels'
import type { LaidOutEdge } from './layout'
import { fitText, textWidth } from './text'

interface EdgePathProps {
  item: LaidOutEdge
  markerPrefix: string
  emphasised: boolean
  dimmed: boolean
}

const PILL_HEIGHT = 18
const PILL_PAD = 6
const MAX_LABEL_WIDTH = 120

/** One routed edge: hairline path, open arrowhead, and a label pill at its midpoint. */
export function EdgePath({ item, markerPrefix, emphasised, dimmed }: EdgePathProps) {
  const meta = EDGE_STYLE[item.style]
  const dashed = item.dashed || meta.dashed
  const label = item.edge.label?.trim() ?? ''
  const shown = label ? fitText(label, MAX_LABEL_WIDTH, 11) : ''
  const pillWidth = shown ? textWidth(shown, 11) + PILL_PAD * 2 : 0
  const marker = `url(#${markerPrefix}-${emphasised ? 'hover' : item.style})`

  return (
    <g
      data-dim
      style={{ opacity: dimmed ? 0.18 : 1 }}
      className="transition-opacity duration-150 motion-reduce:transition-none"
    >
      <path
        d={item.route.d}
        fill="none"
        markerEnd={marker}
        style={{
          stroke: emphasised ? 'var(--brand)' : meta.color,
          strokeOpacity: emphasised ? 1 : meta.opacity,
          strokeWidth: emphasised ? 1.5 : 1.2,
          strokeDasharray: dashed ? '5 4' : undefined,
          strokeLinecap: 'round',
          strokeLinejoin: 'round',
        }}
      />
      {shown ? (
        <g transform={`translate(${item.route.mid.x} ${item.route.mid.y})`}>
          <rect
            x={-pillWidth / 2}
            y={-PILL_HEIGHT / 2}
            width={pillWidth}
            height={PILL_HEIGHT}
            rx={4}
            style={{ fill: 'var(--card)', stroke: 'var(--border)', strokeWidth: 1 }}
          />
          <text
            textAnchor="middle"
            y={4}
            style={{ fill: emphasised ? 'var(--foreground)' : 'var(--muted-foreground)', fontSize: 11 }}
          >
            {shown}
          </text>
          {item.edge.evidencePath ? <title>{`Evidence: ${item.edge.evidencePath}`}</title> : null}
        </g>
      ) : null}
    </g>
  )
}
