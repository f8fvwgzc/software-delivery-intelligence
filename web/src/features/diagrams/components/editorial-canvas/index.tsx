import { useId, useImperativeHandle, useMemo, useRef, useState } from 'react'

import { cn } from '@/lib/utils'

import { CanvasToolbar } from './canvas-toolbar'
import { EdgePath } from './edge-path'
import { rasterisePng, serialiseSvg } from './export'
import { GroupRegion } from './group-region'
import { layoutDiagram } from './layout'
import { Legend } from './legend'
import { Markers } from './markers'
import { NodeCard } from './node-card'
import type { EditorialCanvasProps } from './types'
import { useViewport } from './use-viewport'

export type { CanvasHandle, EditorialCanvasProps } from './types'

interface CanvasProps extends EditorialCanvasProps {
  direction: 'LR' | 'TB'
}

/**
 * The editorial renderer (CONTRACT.md §17): a DiagramSpec as inline SVG laid
 * out by dagre. This module is the heavy chunk; the page lazy-loads it so
 * dagre never lands in the main bundle.
 */
export default function EditorialCanvas({ spec, direction, title, onOpenPath, ref, className }: CanvasProps) {
  const layout = useMemo(() => layoutDiagram(spec, direction), [spec, direction])
  const containerRef = useRef<HTMLDivElement>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const markerPrefix = `m${useId().replace(/[^a-zA-Z0-9]/g, '')}`
  const { viewport, dragging, fitView, zoomBy, wasPan, pointerHandlers } = useViewport(containerRef, layout.bounds)
  const [hovered, setHovered] = useState<string | null>(null)

  // Hovering a node lights its edges and neighbours and fades everything else.
  const connected = useMemo(() => {
    if (hovered === null) return null
    const nodes = new Set<string>([hovered])
    const edges = new Set<string>()
    for (const item of layout.edges) {
      if (item.edge.source === hovered || item.edge.target === hovered) {
        edges.add(item.id)
        nodes.add(item.edge.source)
        nodes.add(item.edge.target)
      }
    }
    return { nodes, edges }
  }, [hovered, layout.edges])

  useImperativeHandle(
    ref,
    () => ({
      fitView,
      exportSvg: () => {
        const svg = svgRef.current
        if (!svg) throw new Error('The canvas is not mounted.')
        return serialiseSvg(svg, layout.bounds, title)
      },
      exportPng: () => {
        const svg = svgRef.current
        if (!svg) return Promise.reject(new Error('The canvas is not mounted.'))
        return rasterisePng(serialiseSvg(svg, layout.bounds, title), layout.bounds)
      },
    }),
    [fitView, layout.bounds, title],
  )

  const { bounds } = layout

  return (
    <div ref={containerRef} className={cn('relative size-full overflow-hidden bg-card', className)}>
      <svg
        ref={svgRef}
        className={cn('size-full touch-none select-none outline-none', dragging ? 'cursor-grabbing' : 'cursor-grab')}
        style={{ fontFamily: 'var(--font-sans)' }}
        role="img"
        aria-label={title}
        {...pointerHandlers}
      >
        <title>{title}</title>
        <Markers prefix={markerPrefix} />
        <g data-content transform={`translate(${viewport.x} ${viewport.y}) scale(${viewport.k})`}>
          <rect x={bounds.x} y={bounds.y} width={bounds.w} height={bounds.h} style={{ fill: 'var(--card)' }} />
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
            <NodeCard
              key={item.node.id}
              item={item}
              emphasised={connected?.nodes.has(item.node.id) ?? false}
              dimmed={connected !== null && !connected.nodes.has(item.node.id)}
              onHover={setHovered}
              onOpen={onOpenPath}
              wasPan={wasPan}
            />
          ))}
          {layout.stylesUsed.length > 1 ? (
            <Legend styles={layout.stylesUsed} bounds={bounds} markerPrefix={markerPrefix} />
          ) : null}
        </g>
      </svg>
      <CanvasToolbar
        zoom={viewport.k}
        onZoomIn={() => zoomBy(1.25)}
        onZoomOut={() => zoomBy(0.8)}
        onFit={fitView}
      />
    </div>
  )
}
