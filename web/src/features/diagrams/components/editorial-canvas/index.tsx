import { useId, useImperativeHandle, useMemo, useRef, useState } from 'react'

import { cn } from '@/lib/utils'

import type { DiagramSpec } from '../../api/queries'
import { CanvasToolbar } from './canvas-toolbar'
import { ErScene, erLegend } from './er/scene'
import { layoutEr, type ErLayout } from './er/layout'
import { rasterisePng, serialiseSvg } from './export'
import { layoutGraph, type GraphLayout } from './graph/layout'
import { GraphScene, graphLegend } from './graph/scene'
import { edgeKey } from './keys'
import { Legend, layoutLegend, type LegendItem } from './legend'
import { Markers } from './markers'
import { layoutSequence, type SequenceLayout } from './sequence/layout'
import { SequenceScene, sequenceLegend } from './sequence/scene'
import type { Box, Direction, EditorialCanvasProps } from './types'
import { useViewport } from './use-viewport'

export type { CanvasHandle, EditorialCanvasProps } from './types'

type Scene =
  | { renderer: 'graph'; layout: GraphLayout }
  | { renderer: 'er'; layout: ErLayout }
  | { renderer: 'sequence'; layout: SequenceLayout }

const CANVAS_MARGIN = 24
/** A one-table schema still gets a legend that fits on two rows. */
const MIN_LEGEND_WIDTH = 360

function buildScene(spec: DiagramSpec, kind: string, direction: Direction): Scene {
  if (kind === 'db_schema') return { renderer: 'er', layout: layoutEr(spec) }
  if (kind === 'sequence') return { renderer: 'sequence', layout: layoutSequence(spec) }
  return { renderer: 'graph', layout: layoutGraph(spec, direction) }
}

function legendFor(scene: Scene, prefix: string): LegendItem[] {
  switch (scene.renderer) {
    case 'er':
      return erLegend(scene.layout, prefix)
    case 'sequence':
      return sequenceLegend(scene.layout, prefix)
    default:
      return graphLegend(scene.layout, prefix)
  }
}

/**
 * The editorial renderer (CONTRACT.md §17): a DiagramSpec as inline SVG.
 * The diagram kind picks the renderer — shaped graph, ER or sequence —
 * and this shell owns what they share: pan/zoom, hover, the legend strip
 * and export. This module is the heavy chunk; the page lazy-loads it so
 * dagre never lands in the main bundle.
 */
export default function EditorialCanvas({ spec, kind, direction, title, onOpenPath, ref, className }: EditorialCanvasProps) {
  const scene = useMemo(() => buildScene(spec, kind, direction), [spec, kind, direction])
  const containerRef = useRef<HTMLDivElement>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const markerPrefix = `m${useId().replace(/[^a-zA-Z0-9]/g, '')}`
  const [hovered, setHovered] = useState<string | null>(null)

  const legendItems = useMemo(() => legendFor(scene, markerPrefix), [scene, markerPrefix])
  const { legend, bounds } = useMemo(() => {
    const { content } = scene.layout
    const width = Math.max(content.w, MIN_LEGEND_WIDTH)
    const legend = layoutLegend(legendItems, width)
    const bounds: Box = {
      x: content.x - CANVAS_MARGIN,
      y: content.y - CANVAS_MARGIN,
      w: width + CANVAS_MARGIN * 2,
      h: content.h + CANVAS_MARGIN * 2 + legend.height,
    }
    return { legend, bounds }
  }, [scene, legendItems])

  const { viewport, canvasHeight, dragging, fitView, zoomBy, wasPan, pointerHandlers } = useViewport(containerRef, bounds)

  // Hovering a node lights its edges and neighbours and fades everything else.
  const connected = useMemo(() => {
    if (hovered === null) return null
    const nodes = new Set<string>([hovered])
    const edges = new Set<string>()
    ;(spec.edges ?? []).forEach((edge, index) => {
      if (edge.source === hovered || edge.target === hovered) {
        edges.add(edgeKey(edge, index))
        nodes.add(edge.source)
        nodes.add(edge.target)
      }
    })
    return { nodes, edges }
  }, [hovered, spec.edges])

  useImperativeHandle(
    ref,
    () => ({
      fitView,
      exportSvg: () => {
        const svg = svgRef.current
        if (!svg) throw new Error('The canvas is not mounted.')
        return serialiseSvg(svg, bounds, title)
      },
      exportPng: () => {
        const svg = svgRef.current
        if (!svg) return Promise.reject(new Error('The canvas is not mounted.'))
        return rasterisePng(serialiseSvg(svg, bounds, title), bounds)
      },
    }),
    [fitView, bounds, title],
  )

  const sceneProps = { markerPrefix, hovered, connected, onHover: setHovered, onOpen: onOpenPath, wasPan }

  return (
    <div className={cn('relative size-full overflow-hidden bg-background', className)}>
      {/* The pane scrolls vertically; the svg is as tall as the drawing at the current zoom. */}
      <div ref={containerRef} className="size-full overflow-x-hidden overflow-y-auto">
        <svg
          ref={svgRef}
          className={cn('block w-full touch-pan-y select-none outline-none', dragging ? 'cursor-grabbing' : 'cursor-grab')}
          style={{ fontFamily: 'var(--font-sans)', height: canvasHeight }}
          role="img"
          aria-label={title}
          {...pointerHandlers}
        >
          <title>{title}</title>
          <Markers prefix={markerPrefix} />
          <g data-content transform={`translate(${viewport.x} ${viewport.y}) scale(${viewport.k})`}>
            <rect x={bounds.x} y={bounds.y} width={bounds.w} height={bounds.h} style={{ fill: 'var(--background)' }} />
            {scene.renderer === 'er' ? (
              <ErScene layout={scene.layout} {...sceneProps} />
            ) : scene.renderer === 'sequence' ? (
              <SequenceScene layout={scene.layout} {...sceneProps} />
            ) : (
              <GraphScene layout={scene.layout} {...sceneProps} />
            )}
            <Legend layout={legend} bounds={bounds} inset={CANVAS_MARGIN} />
          </g>
        </svg>
      </div>
      <CanvasToolbar
        zoom={viewport.k}
        onZoomIn={() => zoomBy(1.25)}
        onZoomOut={() => zoomBy(0.8)}
        onFit={fitView}
      />
    </div>
  )
}
