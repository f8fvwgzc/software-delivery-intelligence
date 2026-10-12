import { lazy, Suspense, useEffect, useState } from 'react'

import { CanvasSkeleton } from '@/features/diagrams/components/canvas-skeleton'

import { Metrics, Scenario, type Done } from '../scenario'
import { syntheticDiagram } from '../synthetic'

export const DIAGRAM_NODES = 34
export const DIAGRAM_EDGES = 48

const EditorialCanvas = lazy(() => import('@/features/diagrams/components/editorial-canvas'))


type LayoutTimings = Record<'layout TB ms' | 'layout LR ms', number>

/** Times both layouts on the main thread through the real layout module (the heavy chunk). */
async function timeLayouts(spec: ReturnType<typeof syntheticDiagram>): Promise<LayoutTimings> {
  const { layoutGraph } = await import('@/features/diagrams/components/editorial-canvas/graph/layout')
  const tb = performance.now()
  layoutGraph(spec, 'TB')
  const tbMs = performance.now() - tb
  const lr = performance.now()
  layoutGraph(spec, 'LR')
  const lrMs = performance.now() - lr
  return { 'layout TB ms': Math.round(tbMs * 10) / 10, 'layout LR ms': Math.round(lrMs * 10) / 10 }
}

function DiagramBody({ done }: { done: Done }) {
  const [spec] = useState(() => syntheticDiagram(DIAGRAM_NODES, DIAGRAM_EDGES))
  const [timings, setTimings] = useState<LayoutTimings | null>(null)

  useEffect(() => {
    let active = true
    void timeLayouts(spec).then((result) => {
      if (!active) return
      setTimings(result)
      done(
        { nodes: spec.nodes?.length ?? 0, edges: spec.edges?.length ?? 0, ...(result as Record<string, number>) },
        { 'TB layout < 100 ms': result['layout TB ms'] < 100, 'LR layout < 100 ms': result['layout LR ms'] < 100 },
      )
    })
    return () => {
      active = false
    }
    // Once per spec.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [spec])

  return (
    <div className="flex flex-col gap-3">
      <div className="relative h-[480px] overflow-hidden rounded-lg bg-background ring-1 ring-border">
        <Suspense fallback={<CanvasSkeleton className="h-full rounded-none ring-0" />}>
          <EditorialCanvas spec={spec} kind="architecture" direction="TB" title="Stress diagram" />
        </Suspense>
      </div>
      {timings ? <Metrics values={timings as Record<string, number>} /> : null}
    </div>
  )
}

export function DiagramScenario() {
  return (
    <Scenario id="diagram" title={`Diagram with ${DIAGRAM_NODES} nodes and ${DIAGRAM_EDGES} edges`} description="The editorial renderer (dagre for LR, stacked bands for TB); both layouts timed on the main thread.">
      {(done) => <DiagramBody done={done} />}
    </Scenario>
  )
}
