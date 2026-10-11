import { NODE_SHAPE_LABEL } from '../../../lib/labels'
import { GroupRegion } from '../group-region'
import { SAMPLE_HEIGHT, SAMPLE_WIDTH, type LegendItem } from '../legend'
import { LINE_COLOR, LINE_OPACITY, markerUrl } from '../markers'
import { FILL_ALPHA, NEUTRAL_TINT, STROKE_ALPHA } from '../palette'
import type { SceneProps } from '../types'
import { DASH, EdgePath } from './edge-path'
import type { GraphLayout } from './layout'
import { GraphNode } from './node'
import { ShapeOutline } from './node-shape'

interface GraphSceneProps extends SceneProps {
  layout: GraphLayout
}

/** Groups under edges under nodes. */
export function GraphScene({ layout, markerPrefix, connected, onHover, onOpen, wasPan }: GraphSceneProps) {
  return (
    <>
      {layout.groups.map((item) => (
        <GroupRegion key={item.group.id} item={item} />
      ))}
      {layout.edges.map((item) => (
        <EdgePath
          key={item.id}
          item={item}
          markerPrefix={markerPrefix}
          emphasised={connected?.edges.has(item.id) ?? false}
          dimmed={connected !== null && !connected.edges.has(item.id)}
        />
      ))}
      {layout.nodes.map((item) => (
        <GraphNode
          key={item.node.id}
          item={item}
          emphasised={connected?.nodes.has(item.node.id) ?? false}
          dimmed={connected !== null && !connected.nodes.has(item.node.id)}
          onHover={onHover}
          onOpen={onOpen}
          wasPan={wasPan}
        />
      ))}
    </>
  )
}

/** A legend line sample: solid or dashed, with the open arrowhead. */
export function edgeSample(prefix: string, dashed: boolean) {
  return (
    <path
      d={`M 0 ${SAMPLE_HEIGHT / 2} H ${SAMPLE_WIDTH - 2}`}
      fill="none"
      markerEnd={markerUrl(prefix, 'open', 'line')}
      style={{ stroke: LINE_COLOR, strokeOpacity: LINE_OPACITY, strokeWidth: 1.2, strokeDasharray: dashed ? DASH : undefined }}
    />
  )
}

/** The shapes on the canvas, then the line styles. */
export function graphLegend(layout: GraphLayout, prefix: string): LegendItem[] {
  const items: LegendItem[] = layout.shapesUsed.map((shape) => {
    const w = shape === 'circle' ? SAMPLE_HEIGHT : SAMPLE_WIDTH
    return {
      id: `shape-${shape}`,
      label: NODE_SHAPE_LABEL[shape],
      sample: (
        <g transform={`translate(${(SAMPLE_WIDTH - w) / 2} 0)`}>
          <ShapeOutline
            shape={shape}
            w={w}
            h={SAMPLE_HEIGHT}
            style={{
              fill: NEUTRAL_TINT.hue,
              fillOpacity: FILL_ALPHA,
              stroke: NEUTRAL_TINT.hue,
              strokeOpacity: STROKE_ALPHA,
              strokeWidth: 1,
            }}
          />
        </g>
      ),
    }
  })
  if (layout.solidEdges) items.push({ id: 'edge-solid', label: 'Observed in code', sample: edgeSample(prefix, false) })
  if (layout.dashedEdges) items.push({ id: 'edge-dashed', label: 'Inferred or returns', sample: edgeSample(prefix, true) })
  return items
}
