import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { WaypointsIcon } from 'lucide-react'
import { Suspense, lazy, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState, useTransition } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'

import { routes } from '@/app/router/routes'
import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Button } from '@/components/ui/button'
import { useIsMobile } from '@/hooks/use-mobile'
import { cn } from '@/lib/utils'

import { graphQueries } from '../api/queries'
import { useGraphLiveReload } from '../api/use-graph-live-reload'
import type { CanvasHandle, FocusRequest } from '../components/architecture-canvas/types'
import { BOARD_HEADER_INSET, BoardHeader } from '../components/board-header'
import { DetailsPanel } from '../components/details-panel'
import { CANVAS_HEIGHT_CLASS, CanvasSkeleton, GraphSkeleton } from '../components/graph-skeleton'
import { GraphToolbar } from '../components/graph-toolbar'
import { NodeDetails } from '../components/node-details'
import { NodeConnections } from '../components/node-details/node-connections'
import { ScopeSummary } from '../components/scope-summary'
import { buildBoard, buildHierarchy, homeScope, type BoardCard, type BoardScope } from '../lib/board'
import { NODE_KINDS } from '../lib/labels'
import type { ImpactHighlight, NodeKind } from '../lib/types'

// React Flow and ELK live in their own chunk. Loading starts as soon as this
// page chunk evaluates, in parallel with the graph query, so the canvas is
// usually ready before the data is.
const loadCanvas = () => import('../components/architecture-canvas')
const ArchitectureCanvas = lazy(loadCanvas)
void loadCanvas()

/** URL state: the board shown (a node key; absent for the overview) and the selected node (a key). */
const FOCUS_PARAM = 'focus'
const NODE_PARAM = 'node'

const NO_FRESH: ReadonlySet<string> = new Set()

interface WorkspaceProps {
  projectId: string
}

/**
 * Node ids that joined the graph since the page opened: a scan finished and
 * the live stream refetched the graph. They wear a "New" tag until the next
 * change, so a map left open shows what the scan added.
 */
function useFreshIds(projectId: string, nodeIds: readonly string[], updatedAt: number) {
  const seen = useRef<{ projectId: string; ids: ReadonlySet<string>; updatedAt: number } | null>(null)
  const [fresh, setFresh] = useState<ReadonlySet<string>>(NO_FRESH)

  useEffect(() => {
    const previous = seen.current
    // An older cached answer (the other history variant, shown while it
    // refetches) says nothing about what a scan added.
    if (previous && previous.projectId === projectId && updatedAt < previous.updatedAt) return
    const ids = new Set(nodeIds)
    seen.current = { projectId, ids, updatedAt }
    if (!previous || previous.projectId !== projectId) {
      setFresh(NO_FRESH)
      return
    }
    const added = nodeIds.filter((id) => !previous.ids.has(id))
    if (added.length > 0) setFresh(new Set(added))
  }, [projectId, nodeIds, updatedAt])

  return fresh
}

/** Everything below the page header: toolbar, the map and the details panel. */
function ArchitectureWorkspace({ projectId }: WorkspaceProps) {
  const [showHistory, setShowHistory] = useState(false)
  const [historyPending, startHistoryTransition] = useTransition()
  const { data: graph, dataUpdatedAt } = useSuspenseQuery(graphQueries.graph(projectId, showHistory))
  useGraphLiveReload(projectId)
  const [searchParams, setSearchParams] = useSearchParams()
  const isMobile = useIsMobile()

  const [selectedKinds, setSelectedKinds] = useState<NodeKind[] | null>(null)
  const [search, setSearch] = useState('')
  const deferredSearch = useDeferredValue(search)
  /** Unfolded groups, as `<board>|<kind>`. */
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set())
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null)
  /** Phones: the details sheet opens on request, apart from the selection, so taps reach the map. */
  const [sheetOpen, setSheetOpen] = useState(false)
  const canvasRef = useRef<CanvasHandle>(null)

  const kindsPresent = useMemo(() => {
    const present = new Set(graph.nodes.map((node) => node.kind))
    return NODE_KINDS.filter((kind) => present.has(kind))
  }, [graph.nodes])
  const activeKinds = selectedKinds ?? kindsPresent
  const hierarchy = useMemo(() => buildHierarchy(graph, new Set(activeKinds)), [graph, activeKinds])

  const nodeIds = useMemo(() => graph.nodes.map((node) => node.id), [graph.nodes])
  const freshIds = useFreshIds(projectId, nodeIds, dataUpdatedAt)
  /** Every node, filtered or not: the URL and the impact list may name a node whose kind is hidden. */
  const graphById = useMemo(() => new Map(graph.nodes.map((node) => [node.id, node])), [graph.nodes])

  const focusKey = searchParams.get(FOCUS_PARAM)
  const focusNode = focusKey === null ? undefined : hierarchy.byKey.get(focusKey)
  const selectedKey = searchParams.get(NODE_PARAM)
  const selectedNode = (selectedKey === null ? undefined : hierarchy.byKey.get(selectedKey)) ?? null
  const selectedId = selectedNode?.id ?? null

  const query = deferredSearch.trim()
  const scopeType: BoardScope['type'] = query !== '' ? 'search' : focusNode ? 'focus' : 'overview'
  const focusId = focusNode?.id ?? null
  const groupScope = scopeType === 'focus' ? (focusId ?? 'overview') : scopeType
  const board = useMemo(() => {
    const scope: BoardScope =
      scopeType === 'search' ? { type: 'search', query } : scopeType === 'focus' && focusId ? { type: 'focus', nodeId: focusId } : { type: 'overview' }
    const kinds = new Set<NodeKind>()
    for (const entry of expanded) {
      const [owner, kind] = entry.split('|')
      if (owner === groupScope) kinds.add(kind as NodeKind)
    }
    return buildBoard(hierarchy, scope, kinds)
  }, [hierarchy, scopeType, query, focusId, groupScope, expanded])

  const impact = useQuery({
    ...graphQueries.impact(projectId, selectedId ?? ''),
    enabled: selectedId !== null,
  })

  const highlight = useMemo<ImpactHighlight | null>(() => {
    if (selectedId === null || !impact.data) return null
    const ids = new Set<string>()
    for (const item of impact.data.impacted) {
      if (item.node) ids.add(item.node.id)
    }
    return { originId: selectedId, ids }
  }, [selectedId, impact.data])

  const keyOf = useCallback((id: string | null) => (id === null ? null : (graphById.get(id)?.key ?? null)), [graphById])

  const writeParams = useCallback(
    (focus: string | null, node: string | null, replace: boolean) => {
      setSearchParams(
        (previous) => {
          const next = new URLSearchParams(previous)
          if (focus === null) next.delete(FOCUS_PARAM)
          else next.set(FOCUS_PARAM, focus)
          if (node === null) next.delete(NODE_PARAM)
          else next.set(NODE_PARAM, node)
          return next
        },
        { replace },
      )
    },
    [setSearchParams],
  )

  /** Selecting stays on the same board and replaces the history entry. */
  const select = useCallback((nodeId: string | null) => writeParams(focusKey, keyOf(nodeId), true), [writeParams, focusKey, keyOf])

  /**
   * Moving between boards is a history entry, so Back walks the drill path.
   * The node to select is located once its board is on screen; the codebase
   * itself has no card (the overview stands for it), so it is only selected.
   */
  const goTo = useCallback(
    (boardNodeId: string | null, selectNodeId: string | null) => {
      setSearch('')
      setSheetOpen(false)
      writeParams(keyOf(boardNodeId), keyOf(selectNodeId), false)
      if (selectNodeId === null || selectNodeId === hierarchy.codebaseId) return
      const boardKey = boardNodeId === null || boardNodeId === hierarchy.codebaseId ? 'overview' : `focus:${boardNodeId}`
      setFocusRequest((previous) => ({ nodeId: selectNodeId, boardKey, nonce: (previous?.nonce ?? 0) + 1 }))
    },
    [writeParams, keyOf, hierarchy.codebaseId],
  )

  const focusHandled = useCallback((nonce: number) => {
    setFocusRequest((previous) => (previous?.nonce === nonce ? null : previous))
  }, [])

  /** Shows a node in its own place: the board of its container, selected, located and flashed. */
  const reveal = useCallback(
    (nodeId: string) => {
      const node = graphById.get(nodeId)
      if (!node) return
      // Its kind is filtered out (an impact row can name it): turn that kind back on first.
      let tree = hierarchy
      if (!hierarchy.byId.has(nodeId)) {
        const kinds = new Set<NodeKind>([...activeKinds, node.kind])
        setSelectedKinds(NODE_KINDS.filter((kind) => kinds.has(kind)))
        tree = buildHierarchy(graph, kinds)
      }
      const home = homeScope(tree, nodeId)
      const homeKey = home.type === 'focus' ? home.nodeId : 'overview'
      // Folded into a group on its home board: unfold that group first.
      const folded = buildBoard(tree, home).cards.some((card) => card.role === 'group' && card.members.includes(nodeId))
      if (folded) setExpanded((previous) => new Set(previous).add(`${homeKey}|${node.kind}`))
      goTo(home.type === 'focus' ? home.nodeId : null, nodeId)
    },
    [graphById, hierarchy, activeKinds, graph, goTo],
  )

  const openCard = useCallback(
    (card: BoardCard) => {
      if (card.role === 'group') {
        setExpanded((previous) => new Set(previous).add(`${groupScope}|${card.kind}`))
        return
      }
      if (!card.node) return
      if (card.role === 'used-by' || card.role === 'uses' || board.scope.type === 'search') reveal(card.node.id)
      else if (card.childCount > 0) goTo(card.node.id, null)
      else select(card.node.id)
    },
    [groupScope, board.scope.type, reveal, goTo, select],
  )

  const up = useCallback(() => {
    if (board.scope.type === 'search') {
      setSearch('')
      return
    }
    if (board.trail.length < 2) return
    const parent = board.trail[board.trail.length - 2]
    // Back up a level with the card you came from selected, so you see where you were.
    goTo(parent.nodeId, board.scope.type === 'focus' ? board.scope.nodeId : null)
  }, [board, goTo])

  const goToCrumb = useCallback(
    (index: number) => {
      const crumb = board.trail[index]
      if (!crumb) return
      goTo(crumb.nodeId, board.scope.type === 'search' ? null : (board.trail[index + 1]?.nodeId ?? null))
    },
    [board, goTo],
  )

  const pickFromPanel = useCallback(
    (card: BoardCard) => {
      if (card.role === 'group' || !card.node) {
        openCard(card)
        return
      }
      select(card.node.id)
      canvasRef.current?.focusNode(card.node.id)
    },
    [openCard, select],
  )

  const filtersActive = selectedKinds !== null || search !== ''
  const clearFilters = () => {
    setSelectedKinds(null)
    setSearch('')
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

  const panelOpen = isMobile ? sheetOpen : true
  const panelTitle = selectedNode ? 'Node' : board.scope.type === 'overview' ? 'Overview' : board.scope.type === 'search' ? 'Search' : 'Details'

  return (
    <div className="flex flex-col gap-3">
      <GraphToolbar
        kinds={kindsPresent}
        selectedKinds={activeKinds}
        onKindsChange={setSelectedKinds}
        search={search}
        onSearchChange={setSearch}
        showHistory={showHistory}
        onShowHistoryChange={(value) => startHistoryTransition(() => setShowHistory(value))}
        historyPending={historyPending}
        visibleCount={hierarchy.byId.size}
        totalCount={graph.nodes.length}
        freshCount={freshIds.size}
        onFitView={() => canvasRef.current?.fitView()}
      />

      <div className="flex items-stretch gap-3">
        {/* `isolate`: the board header's z-index stays inside the canvas, under the sticky project band. */}
        <div className={cn('relative isolate min-w-0 flex-1 overflow-hidden rounded-lg bg-card ring-1 ring-border', CANVAS_HEIGHT_CLASS)}>
          {board.cards.length === 0 ? (
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
            <>
              <BoardHeader
                board={board}
                onCrumb={goToCrumb}
                onUp={up}
                onClearSearch={() => setSearch('')}
                onShowDetails={isMobile ? () => setSheetOpen(true) : undefined}
              />
              <Suspense fallback={<CanvasSkeleton className="h-full rounded-none ring-0" />}>
                <ArchitectureCanvas
                  ref={canvasRef}
                  board={board}
                  selectedId={selectedId}
                  highlight={highlight}
                  freshIds={freshIds}
                  focusRequest={focusRequest}
                  topInset={BOARD_HEADER_INSET}
                  onSelect={select}
                  onOpen={openCard}
                  onUp={up}
                  onFocusHandled={focusHandled}
                />
              </Suspense>
            </>
          )}
        </div>

        <DetailsPanel
          open={panelOpen}
          onClose={() => (isMobile ? setSheetOpen(false) : select(null))}
          title={panelTitle}
          closable={selectedNode !== null}
        >
          {selectedNode ? (
            <NodeDetails projectId={projectId} node={selectedNode} impact={impact} onSelectNode={reveal}>
              <NodeConnections hierarchy={hierarchy} nodeId={selectedNode.id} onOpen={(id) => goTo(id, null)} onReveal={reveal} />
            </NodeDetails>
          ) : (
            <ScopeSummary board={board} hierarchy={hierarchy} onPick={pickFromPanel} onSelectNode={select} />
          )}
        </DetailsPanel>
      </div>
    </div>
  )
}

export default function ArchitecturePage() {
  const { projectId = '' } = useParams()

  return (
    <Page>
      <PageHeader
        title="Architecture"
        description="The parts the scan found and how they connect. Double-click a part (or press Enter twice) to open it; select one to see what a change to it would touch."
      />
      <QueryBoundary fallback={<GraphSkeleton />}>
        <ArchitectureWorkspace projectId={projectId} />
      </QueryBoundary>
    </Page>
  )
}
