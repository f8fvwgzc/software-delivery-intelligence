import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { WaypointsIcon } from 'lucide-react'
import { Suspense, lazy, useDeferredValue, useMemo, useRef, useState, useTransition, type KeyboardEvent } from 'react'
import { Link, useParams } from 'react-router'

import { routes } from '@/app/router/routes'
import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

import { graphQueries } from '../api/queries'
import type { CanvasHandle } from '../components/architecture-canvas/types'
import { DetailsPanel } from '../components/details-panel'
import { CANVAS_HEIGHT_CLASS, CanvasSkeleton, GraphSkeleton } from '../components/graph-skeleton'
import { GraphToolbar } from '../components/graph-toolbar'
import { NodeDetails } from '../components/node-details'
import { filterGraph } from '../lib/filter'
import { NODE_KINDS } from '../lib/labels'
import type { ImpactHighlight, NodeKind } from '../lib/types'

// React Flow and dagre live in their own chunk. Loading starts as soon as this
// page chunk evaluates, in parallel with the graph query, so the canvas is
// usually ready before the data is.
const loadCanvas = () => import('../components/architecture-canvas')
const ArchitectureCanvas = lazy(loadCanvas)
void loadCanvas()

interface WorkspaceProps {
  projectId: string
}

/** Everything below the page header: toolbar, canvas and the details panel. */
function ArchitectureWorkspace({ projectId }: WorkspaceProps) {
  const [showHistory, setShowHistory] = useState(false)
  const [historyPending, startHistoryTransition] = useTransition()
  const { data: graph } = useSuspenseQuery(graphQueries.graph(projectId, showHistory))

  const [selectedKinds, setSelectedKinds] = useState<NodeKind[] | null>(null)
  const [search, setSearch] = useState('')
  const deferredSearch = useDeferredValue(search)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const canvasRef = useRef<CanvasHandle>(null)

  const impact = useQuery({
    ...graphQueries.impact(projectId, selectedId ?? ''),
    enabled: selectedId !== null,
  })

  const kindsPresent = useMemo(() => {
    const present = new Set(graph.nodes.map((node) => node.kind))
    return NODE_KINDS.filter((kind) => present.has(kind))
  }, [graph.nodes])
  const activeKinds = selectedKinds ?? kindsPresent

  const { visibleNodes, visibleEdges } = useMemo(
    () => filterGraph(graph, activeKinds, deferredSearch),
    [graph, activeKinds, deferredSearch],
  )

  const selectedNode = selectedId === null ? null : (graph.nodes.find((node) => node.id === selectedId) ?? null)

  const highlight = useMemo<ImpactHighlight | null>(() => {
    if (selectedId === null || !impact.data) return null
    const ids = new Set<string>()
    for (const item of impact.data.impacted) {
      if (item.node) ids.add(item.node.id)
    }
    return { originId: selectedId, ids }
  }, [selectedId, impact.data])

  const filtersActive = selectedKinds !== null || search !== ''

  const clearFilters = () => {
    setSelectedKinds(null)
    setSearch('')
  }

  const selectFromPanel = (id: string) => {
    setSelectedId(id)
    canvasRef.current?.focusNode(id)
  }

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape' && selectedId !== null) {
      setSelectedId(null)
    }
  }

  if (graph.nodes.length === 0) {
    return (
      <EmptyState
        icon={WaypointsIcon}
        title="No graph yet"
        description="The architecture graph is built from the first successful scan. Nodes and relations appear once the repository has been analysed."
        action={
          <Button asChild variant="outline" size="sm">
            <Link to={routes.project(projectId)}>Open project</Link>
          </Button>
        }
      />
    )
  }

  return (
    <div className="flex flex-col gap-3" onKeyDown={handleKeyDown}>
      <GraphToolbar
        kinds={kindsPresent}
        selectedKinds={activeKinds}
        onKindsChange={setSelectedKinds}
        search={search}
        onSearchChange={setSearch}
        showHistory={showHistory}
        onShowHistoryChange={(value) => startHistoryTransition(() => setShowHistory(value))}
        historyPending={historyPending}
        visibleCount={visibleNodes.length}
        totalCount={graph.nodes.length}
        onFitView={() => canvasRef.current?.fitView()}
      />

      <div className="flex items-stretch gap-3">
        <div className={cn('relative min-w-0 flex-1 overflow-hidden rounded-lg bg-card ring-1 ring-border', CANVAS_HEIGHT_CLASS)}>
          {visibleNodes.length === 0 ? (
            <EmptyState
              title="No nodes match"
              description={filtersActive ? 'Try another search or turn kinds back on.' : 'Nothing to show.'}
              action={
                filtersActive ? (
                  <Button variant="outline" size="sm" onClick={clearFilters}>
                    Clear filters
                  </Button>
                ) : undefined
              }
              className="h-full border-0"
            />
          ) : (
            <Suspense fallback={<CanvasSkeleton className="h-full rounded-none ring-0" />}>
              <ArchitectureCanvas
                ref={canvasRef}
                nodes={visibleNodes}
                edges={visibleEdges}
                selectedId={selectedId}
                highlight={highlight}
                onSelect={setSelectedId}
              />
            </Suspense>
          )}
        </div>

        <DetailsPanel open={selectedNode !== null} onClose={() => setSelectedId(null)} title="Node">
          {selectedNode ? (
            <NodeDetails projectId={projectId} node={selectedNode} impact={impact} onSelectNode={selectFromPanel} />
          ) : null}
        </DetailsPanel>
      </div>
    </div>
  )
}

export default function ArchitecturePage() {
  const { projectId = '' } = useParams()

  return (
    <Page wide className="gap-4">
      <PageHeader
        title="Architecture"
        description="What the scan found and how it connects. Select a node to see what a change to it would touch."
      />
      <QueryBoundary fallback={<GraphSkeleton />}>
        <ArchitectureWorkspace projectId={projectId} />
      </QueryBoundary>
    </Page>
  )
}
