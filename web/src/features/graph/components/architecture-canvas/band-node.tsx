import type { NodeProps } from '@xyflow/react'
import { memo } from 'react'

import { BAND_HEIGHT } from './elk-layout'
import type { BandFlowNode } from './types'

/** Half the gap the label leaves in the bracket line: the label's width plus air, never past the ends. */
function labelHalfGap(label: string, width: number) {
  const estimate = label.length * 3.6 + 12
  return Math.max(0, Math.min(estimate, width / 2 - 12))
}

/**
 * The bracket over one band of a focus board ("Used by", "Inside X",
 * "Uses"): a hairline with ends that curl down towards the cards and a gap
 * for the label, drawn in board space so it pans and zooms with the map.
 * Geometry after Floorplan's column headers.
 */
function BandNodeView({ data, width }: NodeProps<BandFlowNode>) {
  const w = width ?? 0
  const y = BAND_HEIGHT / 2
  const centre = w / 2
  const half = labelHalfGap(data.label, w)
  const d = `M0,${y + 9} v-5 q0,-4 4,-4 H${centre - half} M${centre + half},${y} H${w - 4} q4,0 4,4 v5`

  return (
    <div className="arch-band relative size-full" aria-hidden>
      <svg className="absolute inset-0 overflow-visible" width={w} height={BAND_HEIGHT}>
        <path d={d} fill="none" stroke="currentColor" strokeWidth={1.25} strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <span className="absolute inset-x-0 top-0 truncate px-4 text-center text-xs leading-5 font-medium text-muted-foreground">
        {data.label}
      </span>
    </div>
  )
}

export const BandNode = memo(BandNodeView)
