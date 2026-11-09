import {
  Background,
  BackgroundVariant,
  Controls,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  useStore,
  type EdgeTypes,
  type FitViewOptions,
  type NodeTypes,
  type OnNodesChange,
  type ReactFlowState,
} from '@xyflow/react'
import { useCallback, useEffect, useEffectEvent, useImperativeHandle, useMemo, useRef, useState } from 'react'

import '@xyflow/react/dist/style.css'
import './flow.css'

import { cn } from '@/lib/utils'

import { neighbourhood, type BoardCard } from '../../lib/board'
import { NODE_KIND_META } from '../../lib/labels'
import { BandNode } from './band-node'
import { CardActionsContext, CardNode, PillNode } from './card-node'
import { BAND_HEIGHT, type BoardLayout, type CardBox } from './elk-layout'
import type { ArchEdge, ArchNode, ArchitectureCanvasProps, CardEmphasis, PortDot, WireState } from './types'
import { useBoardLayout } from './use-board-layout'
import { WireEdge } from './wire-edge'

const NODE_TYPES: NodeTypes = { card: CardNode, pill: PillNode, band: BandNode }
const EDGE_TYPES: EdgeTypes = { wire: WireEdge }

/** What React Flow tells screen readers about a card, matching how this map works. */
const ARIA_LABELS = {
  'node.a11yDescription.default': 'Press Enter to select, Enter again to open. Backspace goes up a level, F fits the map.',
  'node.a11yDescription.keyboardDisabled': 'Press Enter to select, Enter again to open. Backspace goes up a level, F fits the map.',
}

const MIN_ZOOM = 0.15
const MAX_ZOOM = 1.75
/** Fitting never zooms past this: a small board stays at reading size instead of filling the canvas. */
const FIT_MAX_ZOOM = 1
/** Drilling in zooms towards the card until it fills this share of the canvas, as Floorplan does. */
const DRILL_FILL = 0.6
/** The zoom towards the card, and the fade of the old board under it. */
const DRILL_MS = 200
/** Locate keeps at least this zoom, so the card is readable when the camera arrives. */
const LOCATE_MIN_ZOOM = 0.75
/** Entrance stagger per card, capped so the last card starts within the motion budget. */
const STAGGER_MS = 18
const STAGGER_CAP_MS = 180

function prefersReducedMotion() {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function motion(ms: number) {
  return prefersReducedMotion() ? 0 : ms
}

function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)
}

/** Level of detail, after Floorplan: far out the cards drop their prose and the titles grow. */
const levelOfDetail = (state: ReactFlowState) => {
  const zoom = state.transform[2]
  if (zoom < 0.42) return 'lod-low'
  if (zoom < 0.65) return 'lod-mid'
  return ''
}
const canvasWidth = (state: ReactFlowState) => state.width
const canvasHeight = (state: ReactFlowState) => state.height

/** Flow ids carry the board key, so a new board mounts fresh nodes and their entrance replays. */
const flowId = (layout: BoardLayout, cardId: string) => `${layout.key}|${cardId}`

const isPill = (card: BoardCard) => card.role === 'used-by' || card.role === 'uses'

function CanvasBody({
  board,
  selectedId,
  highlight,
  freshIds,
  focusRequest,
  topInset,
  onSelect,
  onOpen,
  onUp,
  onFocusHandled,
  ref,
}: ArchitectureCanvasProps) {
  const layout = useBoardLayout(board)
  const { fitView, setCenter, getZoom, zoomIn, zoomOut } = useReactFlow<ArchNode, ArchEdge>()
  const width = useStore(canvasWidth)
  const height = useStore(canvasHeight)
  const lod = useStore(levelOfDetail)
  const wrapperRef = useRef<HTMLDivElement>(null)

  /** Hovered card and wire, by flow id: a board change under a resting pointer never leaves a stale hover. */
  const [hoveredCard, setHoveredCard] = useState<string | null>(null)
  const [hoveredWire, setHoveredWire] = useState<string | null>(null)
  /** The old board is fading under the zoom towards an opened card. */
  const [leaving, setLeaving] = useState(false)
  /** The layout most recently fitted to the canvas; until a new board is fitted it stays hidden. */
  const [fitted, setFitted] = useState<BoardLayout | null>(null)
  /** The locate flash, by flow id (board and card), so a card with the same id on another board never replays it. */
  const [flash, setFlash] = useState<{ flowId: string; count: number }>({ flowId: '', count: 0 })
  /**
   * Sizes React Flow measured, written back onto the nodes as a controlled
   * flow must: until a node carries `measured`, React Flow does not count the
   * board as initialised and a queued `fitView` never resolves.
   */
  const [measured, setMeasured] = useState<ReadonlyMap<string, { width: number; height: number }>>(() => new Map())
  const fitting = useRef<BoardLayout | null>(null)
  const drillTimer = useRef<number | undefined>(undefined)
  const handledFocus = useRef(0)

  const fitOptions = useCallback(
    (duration: number): FitViewOptions<ArchNode> => ({
      padding: { top: `${topInset}px`, right: '32px', bottom: '32px', left: '32px' },
      maxZoom: FIT_MAX_ZOOM,
      duration: motion(duration),
    }),
    [topInset],
  )

  // Keyboard focus follows the map into a new board, when it was on the map
  // (or nowhere): onto the selected card (the one you came up from), else the first.
  const focusIntoBoard = useEffectEvent((next: BoardLayout) => {
    const active = document.activeElement
    const wasOnMap = active === null || active === document.body || (wrapperRef.current?.contains(active) ?? false)
    if (!wasOnMap) return
    const target =
      next.cards.find((box) => selectedId !== null && box.card.node?.id === selectedId) ??
      [...next.cards].sort((a, b) => a.order - b.order)[0]
    if (!target) return
    const element = wrapperRef.current?.querySelector<HTMLElement>(`.react-flow__node[data-id="${CSS.escape(flowId(next, target.card.id))}"]`)
    element?.focus({ preventScroll: true })
  })

  // A new layout is fitted as soon as React Flow has its nodes (its fitView
  // queues until then): a different board instantly while it is still hidden
  // (then it fades and rises in), the same board re-laid out (a group
  // unfolded, a scan landed) with a short glide.
  useEffect(() => {
    if (!layout || width === 0 || fitting.current === layout) return
    fitting.current = layout
    const sameBoard = fitted?.key === layout.key
    const duration = sameBoard ? 250 : 0
    // React Flow settles fitView on the glide's end, which never comes if a
    // pan or zoom interrupts it: give up waiting just after the glide.
    const settled = new Promise((resolve) => window.setTimeout(resolve, motion(duration) + 120))
    void Promise.race([fitView(fitOptions(duration)), settled]).then(() => {
      if (fitting.current !== layout) return
      setFitted(layout)
      setLeaving(false)
      if (!sameBoard) focusIntoBoard(layout)
    })
  }, [layout, width, fitted?.key, fitView, fitOptions])

  // The details panel or the window resizing refits the board that is already there.
  const refit = useEffectEvent(() => {
    if (fitted) void fitView(fitOptions(200))
  })
  useEffect(() => {
    if (width > 0 && height > 0) refit()
  }, [width, height])

  // Never stay faded if an open led nowhere (the board did not change).
  useEffect(() => {
    if (!leaving) return
    const timer = window.setTimeout(() => {
      setLeaving(false)
      void fitView(fitOptions(0))
    }, 900)
    return () => window.clearTimeout(timer)
  }, [leaving, fitView, fitOptions])

  useEffect(() => () => window.clearTimeout(drillTimer.current), [])

  const boxForNode = useCallback(
    (nodeId: string): CardBox | undefined =>
      layout?.cards.find((box) => box.card.node?.id === nodeId) ??
      layout?.cards.find((box) => box.card.role === 'group' && box.card.members.includes(nodeId)),
    [layout],
  )

  const locate = useCallback(
    (nodeId: string) => {
      if (!layout) return
      const box = boxForNode(nodeId)
      if (!box) return
      // The card flashes once the camera has arrived on it.
      void setCenter(box.x + box.width / 2, box.y + box.height / 2, {
        zoom: Math.max(getZoom(), LOCATE_MIN_ZOOM),
        duration: motion(250),
      }).then(() => setFlash((previous) => ({ flowId: flowId(layout, box.card.id), count: previous.count + 1 })))
    },
    [layout, boxForNode, setCenter, getZoom],
  )

  // A reveal asks to locate a node once its own board (not the one fading
  // out) is laid out and fitted. The page clears the request once it is used.
  useEffect(() => {
    if (!focusRequest || focusRequest.nonce === handledFocus.current) return
    if (!layout || fitted !== layout || layout.key !== focusRequest.boardKey) return
    handledFocus.current = focusRequest.nonce
    locate(focusRequest.nodeId)
    onFocusHandled(focusRequest.nonce)
  }, [focusRequest, layout, fitted, locate, onFocusHandled])

  useImperativeHandle(
    ref,
    () => ({
      fitView: () => void fitView(fitOptions(250)),
      focusNode: locate,
    }),
    [fitView, fitOptions, locate],
  )

  /** Opening zooms towards the card while the old board fades, then the page changes the board. */
  const openCard = useCallback(
    (card: BoardCard) => {
      const box = layout?.cards.find((candidate) => candidate.card.id === card.id)
      if (card.role === 'group' || !box || prefersReducedMotion()) {
        onOpen(card)
        return
      }
      const zoom = Math.min(MAX_ZOOM, Math.min(width / box.width, height / box.height) * DRILL_FILL)
      void setCenter(box.x + box.width / 2, box.y + box.height / 2, { zoom, duration: DRILL_MS })
      setLeaving(true)
      window.clearTimeout(drillTimer.current)
      drillTimer.current = window.setTimeout(() => onOpen(card), DRILL_MS)
    },
    [layout, width, height, setCenter, onOpen],
  )

  const shownScope = (layout?.board ?? board).scope.type
  const canOpen = useCallback(
    (card: BoardCard) => card.childCount > 0 || isPill(card) || (shownScope === 'search' && card.node !== null),
    [shownScope],
  )

  /** Every card standing for the selected node: a neighbour can sit in both outer bands. */
  const selectedIds = useMemo(
    () => new Set(selectedId === null ? [] : (layout?.cards ?? []).filter((box) => box.card.node?.id === selectedId).map((box) => box.card.id)),
    [layout, selectedId],
  )
  const selectedCard = useMemo(
    () => layout?.cards.find((box) => selectedIds.has(box.card.id))?.card ?? null,
    [layout, selectedIds],
  )


  // Emphasis: the selected card, what it is wired to and what its impact
  // analysis reaches stay; everything else recedes. Hovering a card or a wire
  // only lights its wires, it never dims the rest.
  //
  // Everything here reads the board the displayed layout was made from: while
  // the next board is being laid out, the old one is still on screen.
  const emphasis = useMemo(() => {
    const shown = layout?.board ?? board
    const cardsById = new Map(shown.cards.map((card) => [card.id, card]))
    const related = new Set<string>()
    for (const id of selectedIds) for (const near of neighbourhood(shown, id)) related.add(near)
    const impacted = new Set<string>()
    if (highlight) {
      const reached = new Set([highlight.originId, ...highlight.ids])
      for (const card of shown.cards) if (card.members.some((id) => reached.has(id))) impacted.add(card.id)
    }

    const cardEmphasis = (card: BoardCard): CardEmphasis => {
      if (selectedIds.size === 0) return 'none'
      if (selectedIds.has(card.id)) return 'selected'
      if (impacted.has(card.id)) return 'impacted'
      if (related.has(card.id)) return 'related'
      return 'dimmed'
    }
    const kept = (cardId: string) => {
      const card = cardsById.get(cardId)
      return card !== undefined && cardEmphasis(card) !== 'dimmed'
    }

    // Hover is the live read (the moving dash); the selection's wires stay lit but still.
    const wireState = (wireId: string, source: string, target: string): WireState => {
      if (layout && hoveredWire === flowId(layout, wireId)) return 'active'
      if (layout && hoveredCard !== null && (flowId(layout, source) === hoveredCard || flowId(layout, target) === hoveredCard)) return 'active'
      if (selectedIds.has(source) || selectedIds.has(target)) return 'linked'
      if (selectedIds.size > 0 && !(kept(source) && kept(target))) return 'dimmed'
      return 'none'
    }
    return { cardEmphasis, wireState }
  }, [board, layout, selectedIds, highlight, hoveredCard, hoveredWire])

  const flowNodes = useMemo<ArchNode[]>(() => {
    if (!layout) return []
    const ports = new Map<string, PortDot[]>()
    const boxById = new Map(layout.cards.map((box) => [box.card.id, box]))
    for (const route of layout.wires) {
      const state = emphasis.wireState(route.wire.id, route.wire.source, route.wire.target)
      const ends = [
        { cardId: route.wire.source, point: route.points[0], end: 'source' as const },
        { cardId: route.wire.target, point: route.points[route.points.length - 1], end: 'target' as const },
      ]
      for (const { cardId, point, end } of ends) {
        const box = boxById.get(cardId)
        if (!box) continue
        const list = ports.get(cardId) ?? []
        list.push({ id: `${route.wire.id}:${end}`, x: point.x - box.x, y: point.y - box.y, end, state })
        ports.set(cardId, list)
      }
    }

    const cards: ArchNode[] = layout.cards.map((box) => {
      const { card } = box
      const kind = NODE_KIND_META[card.kind].label
      return {
        id: flowId(layout, card.id),
        type: isPill(card) ? 'pill' : 'card',
        position: { x: box.x, y: box.y },
        width: box.width,
        height: box.height,
        measured: measured.get(flowId(layout, card.id)),
        selected: selectedIds.has(card.id),
        draggable: false,
        connectable: false,
        selectable: card.node !== null,
        ariaLabel: [
          card.label,
          card.role === 'group' ? card.inside : kind,
          card.role === 'used-by' ? 'uses what is shown here, press Enter twice to show it in its place' : null,
          card.role === 'uses' ? 'used by what is shown here, press Enter twice to show it in its place' : null,
          card.role === 'group' ? 'press Enter to show each one' : null,
          card.childCount > 0 && card.role !== 'group' ? `${card.inside}, press Enter twice to open` : null,
        ]
          .filter(Boolean)
          .join(', '),
        data: {
          card,
          emphasis: emphasis.cardEmphasis(card),
          fresh: card.members.some((id) => freshIds.has(id)),
          ports: ports.get(card.id) ?? [],
          delay: Math.min(box.order * STAGGER_MS, STAGGER_CAP_MS),
          flash: flash.flowId === flowId(layout, card.id) ? flash.count : 0,
        },
      }
    })

    const bands: ArchNode[] = layout.bands.map((band) => ({
      id: flowId(layout, `band:${band.id}`),
      type: 'band',
      position: { x: band.x, y: band.y },
      width: band.width,
      height: BAND_HEIGHT,
      measured: measured.get(flowId(layout, `band:${band.id}`)),
      draggable: false,
      connectable: false,
      selectable: false,
      focusable: false,
      data: { label: band.label },
    }))

    return [...bands, ...cards]
  }, [layout, emphasis, selectedIds, freshIds, flash, measured])

  const flowEdges = useMemo<ArchEdge[]>(() => {
    if (!layout) return []
    return layout.wires.map((route) => ({
      id: flowId(layout, route.wire.id),
      type: 'wire',
      source: flowId(layout, route.wire.source),
      target: flowId(layout, route.wire.target),
      selectable: false,
      focusable: false,
      data: {
        points: route.points,
        relations: route.wire.relations,
        category: route.wire.category,
        count: route.wire.count,
        expired: route.wire.expired,
        state: emphasis.wireState(route.wire.id, route.wire.source, route.wire.target),
      },
    }))
  }, [layout, emphasis])

  // Keyboard, after Floorplan: Backspace up, Escape clears then goes up, F fits,
  // Enter opens the selected card, + and - zoom. Only while focus is on the map
  // or nowhere in particular, so the toolbar and panel keep their own keys.
  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return
    if (isTypingTarget(event.target)) return
    const target = event.target as Node | null
    const onMap = target === document.body || (target !== null && wrapperRef.current?.contains(target))
    if (!onMap) return
    let handled = true
    switch (event.key) {
      case 'Backspace':
        onUp()
        break
      case 'Escape':
        if (selectedId !== null) onSelect(null)
        else onUp()
        break
      case 'f':
      case 'F':
        void fitView(fitOptions(250))
        break
      case 'Enter': {
        // The card under keyboard focus: React Flow selects it on the first
        // Enter, the second opens it. A group has no selection, so it opens at once.
        const element = event.target instanceof Element ? event.target.closest('.react-flow__node') : null
        const node = element === event.target ? flowNodes.find((candidate) => candidate.id === element?.getAttribute('data-id')) : undefined
        const focused = node && node.type !== 'band' ? node.data.card : null
        const card = focused ?? (target === document.body ? selectedCard : null)
        if (card && (card.role === 'group' || (selectedIds.has(card.id) && canOpen(card)))) openCard(card)
        else handled = false
        break
      }
      case '+':
      case '=':
        void zoomIn({ duration: motion(150) })
        break
      case '-':
        void zoomOut({ duration: motion(150) })
        break
      default:
        handled = false
    }
    if (handled) event.preventDefault()
  })
  useEffect(() => {
    const listener = (event: KeyboardEvent) => onKeyDown(event)
    window.addEventListener('keydown', listener)
    return () => window.removeEventListener('keydown', listener)
  }, [])

  // Measurements are written back; keyboard selection (Enter or Space on a
  // focused card) arrives as a select change.
  const handleNodesChange: OnNodesChange<ArchNode> = (changes) => {
    const sizes = changes.flatMap((change) =>
      change.type === 'dimensions' && change.dimensions ? [[change.id, change.dimensions] as const] : [],
    )
    if (sizes.length > 0) {
      setMeasured((previous) => {
        const live = new Set(flowNodes.map((node) => node.id))
        const next = new Map([...previous].filter(([id]) => live.has(id)))
        for (const [id, size] of sizes) next.set(id, size)
        return next
      })
    }
    // One keypress selects one card and unselects the last; only the selection counts.
    const selecting = changes.some((change) => change.type === 'select' && change.selected)
    for (const change of changes) {
      if (change.type !== 'select' || (selecting && !change.selected)) continue
      const node = flowNodes.find((candidate) => candidate.id === change.id)
      const nodeId = node && node.type !== 'band' ? node.data.card.node?.id : undefined
      if (nodeId === undefined) continue
      if (change.selected) onSelect(nodeId)
      else if (nodeId === selectedId) onSelect(null)
    }
  }

  const cardOf = (node: ArchNode) => (node.type === 'band' ? null : node.data.card)
  const hidden = leaving || layout === null || fitted?.key !== layout.key
  const actions = useMemo(() => ({ open: openCard }), [openCard])

  return (
    <CardActionsContext value={actions}>
      <div ref={wrapperRef} className="size-full">
        <ReactFlow<ArchNode, ArchEdge>
          className={cn('arch-flow', lod, hidden && 'is-hidden')}
          nodes={flowNodes}
          edges={flowEdges}
          nodeTypes={NODE_TYPES}
          edgeTypes={EDGE_TYPES}
          onNodesChange={handleNodesChange}
          onNodeClick={(_, node) => {
            const card = cardOf(node)
            if (!card) return
            if (card.role === 'group') openCard(card)
            else if (card.node) onSelect(card.node.id)
          }}
          onNodeDoubleClick={(_, node) => {
            const card = cardOf(node)
            if (card && canOpen(card)) openCard(card)
          }}
          onNodeMouseEnter={(_, node) => {
            if (cardOf(node)) setHoveredCard(node.id)
          }}
          onNodeMouseLeave={() => setHoveredCard(null)}
          onEdgeMouseEnter={(_, edge) => setHoveredWire(edge.id)}
          onEdgeMouseLeave={() => setHoveredWire(null)}
          onPaneClick={() => onSelect(null)}
          minZoom={MIN_ZOOM}
          maxZoom={MAX_ZOOM}
          nodesDraggable={false}
          nodesConnectable={false}
          edgesFocusable={false}
          selectNodesOnDrag={false}
          zoomOnDoubleClick={false}
          // The canvas sits in a scrolling page: the wheel keeps scrolling the
          // page, and zoom comes from pinch, the controls or + and -.
          zoomOnScroll={false}
          preventScrolling={false}
          deleteKeyCode={null}
          ariaLabelConfig={ARIA_LABELS}
          multiSelectionKeyCode={null}
          selectionKeyCode={null}
        >
          <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="var(--border)" />
          <Controls position="bottom-right" showInteractive={false} showFitView={false} aria-label="Zoom" />
        </ReactFlow>
      </div>
    </CardActionsContext>
  )
}

/**
 * The architecture map: one board at a time, laid out by ELK and drawn with
 * React Flow. This module is the heavy chunk (React Flow and ELK): the page
 * lazy-loads it, so neither lands in the main bundle.
 */
export default function ArchitectureCanvas(props: ArchitectureCanvasProps) {
  return (
    <ReactFlowProvider>
      <CanvasBody {...props} />
    </ReactFlowProvider>
  )
}
