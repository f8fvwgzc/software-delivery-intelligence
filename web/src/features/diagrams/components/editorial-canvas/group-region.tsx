import type { DiagramGroup } from '../../api/queries'
import { GROUP_FILL_ALPHA, GROUP_STROKE_ALPHA, type Tint } from './palette'
import { fitText } from './text'
import type { Box } from './types'

export interface LaidOutGroup {
  group: DiagramGroup
  box: Box
  tint: Tint
}

/** Padding around a group's nodes; the top also holds the label line. */
export const GROUP_PAD = 16
export const GROUP_LABEL_HEIGHT = 20
export const GROUP_PAD_TOP = GROUP_PAD + GROUP_LABEL_HEIGHT

const LABEL_FONT = { size: 12, weight: 500 }

/** A lightly tinted rectangle behind its nodes, labelled top-centre like a Mermaid cluster. */
export function GroupRegion({ item }: { item: LaidOutGroup }) {
  const { group, box, tint } = item
  return (
    <g>
      <rect
        x={box.x}
        y={box.y}
        width={box.w}
        height={box.h}
        rx={6}
        style={{
          fill: tint.hue,
          fillOpacity: GROUP_FILL_ALPHA,
          stroke: tint.hue,
          strokeOpacity: GROUP_STROKE_ALPHA,
          strokeWidth: 1,
        }}
      />
      <text
        x={box.x + box.w / 2}
        y={box.y + GROUP_PAD / 2 + GROUP_LABEL_HEIGHT / 2}
        textAnchor="middle"
        dominantBaseline="central"
        style={{ fill: tint.fg, fontSize: LABEL_FONT.size, fontWeight: LABEL_FONT.weight }}
      >
        {fitText(group.label, box.w - GROUP_PAD * 2, LABEL_FONT)}
      </text>
      {group.description ? <title>{group.description}</title> : null}
    </g>
  )
}
