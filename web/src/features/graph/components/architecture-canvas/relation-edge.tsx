import { BaseEdge, EdgeLabelRenderer, getBezierPath, useStore, type EdgeProps, type ReactFlowState } from '@xyflow/react'
import { memo } from 'react'

import { cn } from '@/lib/utils'

import { RELATION_STYLE, relationLabel } from '../../lib/labels'
import type { ArchEdge } from './types'

/** Labels only read above this zoom; below it they would be noise. */
const LABEL_ZOOM = 0.7
const labelsVisible = (state: ReactFlowState) => state.transform[2] >= LABEL_ZOOM

export const ARROW_ID = 'arch-arrow'
export const ARROW_ACCENT_ID = 'arch-arrow-accent'

function edgeOpacity(expired: boolean, dimmed: boolean) {
  if (dimmed) return 0.2
  if (expired) return 0.3
  return 1
}

/** Bezier edge styled by relation category, labelled with the relation verb. */
function RelationEdgeView({
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
  selected,
}: EdgeProps<ArchEdge>) {
  const showLabel = useStore(labelsVisible)
  const [path, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  })

  const category = data?.category ?? 'unspecified'
  const style = RELATION_STYLE[category]
  const expired = data?.expired ?? false
  const dimmed = data?.dimmed ?? false
  const opacity = edgeOpacity(expired, dimmed)

  return (
    <>
      <BaseEdge
        path={path}
        markerEnd={`url(#${style.accent ? ARROW_ACCENT_ID : ARROW_ID})`}
        style={{
          stroke: style.accent ? 'var(--brand)' : 'var(--arch-edge)',
          strokeWidth: selected ? 1.5 : 1,
          strokeDasharray: style.dash,
          opacity,
          transition: 'opacity 150ms ease-out',
        }}
      />
      {showLabel && data ? (
        <EdgeLabelRenderer>
          <div
            className={cn(
              'nodrag nopan pointer-events-none absolute rounded-sm bg-background px-1 py-px text-2xs leading-4 text-muted-foreground',
              style.accent && 'text-brand',
              expired && 'line-through',
            )}
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
              opacity,
            }}
          >
            {relationLabel(data.relation)}
          </div>
        </EdgeLabelRenderer>
      ) : null}
    </>
  )
}

export const RelationEdge = memo(RelationEdgeView)
