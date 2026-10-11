import type { LaidOutGroup } from './layout'
import { fitText } from './text'

/** A tinted rounded region behind its nodes with a small-caps label in the top-left corner. */
export function GroupRegion({ item }: { item: LaidOutGroup }) {
  const { group, box } = item
  return (
    <g>
      <rect
        x={box.x}
        y={box.y}
        width={box.w}
        height={box.h}
        rx={8}
        style={{ fill: 'var(--muted)', fillOpacity: 0.4, stroke: 'var(--border)', strokeWidth: 1 }}
      />
      <text
        x={box.x + 12}
        y={box.y + 20}
        style={{
          fill: 'var(--muted-foreground)',
          fontSize: 11,
          fontWeight: 500,
          letterSpacing: '0.06em',
          fontVariantCaps: 'all-small-caps',
        }}
      >
        {fitText(group.label, box.w - 24, 11)}
      </text>
      {group.description ? <title>{group.description}</title> : null}
    </g>
  )
}
