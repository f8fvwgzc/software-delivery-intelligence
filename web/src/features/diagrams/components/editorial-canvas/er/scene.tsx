import { EdgePath } from '../graph/edge-path'
import { edgeSample } from '../graph/scene'
import { GroupRegion } from '../group-region'
import { SAMPLE_HEIGHT, SAMPLE_WIDTH, type LegendItem } from '../legend'
import { LINE_COLOR, LINE_OPACITY, markerUrl, type MarkerShape } from '../markers'
import { paletteTint } from '../palette'
import type { SceneProps } from '../types'
import type { ErLayout } from './layout'
import { KeyBadge, TableCard } from './table-card'

interface ErSceneProps extends SceneProps {
  layout: ErLayout
}

/** Crow's foot at the child (source) table, a bar at the parent. */
const FK_ENDS: { start: MarkerShape; end: MarkerShape } = { start: 'crow', end: 'bar' }

export function ErScene({ layout, markerPrefix, connected, onHover, onOpen, wasPan }: ErSceneProps) {
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
          ends={FK_ENDS}
          emphasised={connected?.edges.has(item.id) ?? false}
          dimmed={connected !== null && !connected.edges.has(item.id)}
        />
      ))}
      {layout.tables.map((item) => (
        <TableCard
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

function endSample(prefix: string, shape: MarkerShape, at: 'start' | 'end') {
  return (
    <path
      d={`M 1 ${SAMPLE_HEIGHT / 2} H ${SAMPLE_WIDTH - 1}`}
      fill="none"
      markerStart={at === 'start' ? markerUrl(prefix, shape, 'line') : undefined}
      markerEnd={at === 'end' ? markerUrl(prefix, shape, 'line') : undefined}
      style={{ stroke: LINE_COLOR, strokeOpacity: LINE_OPACITY, strokeWidth: 1.2 }}
    />
  )
}

/** Cardinality ends, the key badges, then the inferred line style when any edge is dashed. */
export function erLegend(layout: ErLayout, prefix: string): LegendItem[] {
  const items: LegendItem[] = []
  if (layout.edges.length > 0) {
    items.push({ id: 'crow', label: 'Many (holds the foreign key)', sample: endSample(prefix, 'crow', 'start') })
    items.push({ id: 'bar', label: 'One (referenced table)', sample: endSample(prefix, 'bar', 'end') })
  }
  if (layout.hasPk) items.push({ id: 'pk', label: 'Primary key', sample: <KeyBadge kind="PK" x={6} y={SAMPLE_HEIGHT / 2} tint={paletteTint(0)} /> })
  if (layout.hasFk) items.push({ id: 'fk', label: 'Foreign key', sample: <KeyBadge kind="FK" x={6} y={SAMPLE_HEIGHT / 2} tint={paletteTint(0)} /> })
  if (layout.dashedEdges) items.push({ id: 'edge-dashed', label: 'Inferred', sample: edgeSample(prefix, true) })
  return items
}
