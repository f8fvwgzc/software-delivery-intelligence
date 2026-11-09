import ELK from 'elkjs/lib/elk.bundled.js'
import type { ElkExtendedEdge, ElkNode, LayoutOptions } from 'elkjs/lib/elk-api'

import type { Board, BoardCard, BoardWire } from '../../lib/board'

/**
 * ELK layered layout for one board, with the options Floorplan uses
 * (github.com/haydenschmitty/floorplan, MIT): left to right, orthogonal
 * routing, network-simplex placement, model order kept so the same input
 * lays out the same way, and disconnected parts packed apart. Spacing is
 * scaled to this app's smaller cards.
 */
const ROOT_OPTIONS: LayoutOptions = {
  'elk.algorithm': 'layered',
  'elk.direction': 'RIGHT',
  'elk.edgeRouting': 'ORTHOGONAL',
  'elk.layered.spacing.nodeNodeBetweenLayers': '112',
  'elk.spacing.nodeNode': '28',
  'elk.layered.spacing.edgeEdgeBetweenLayers': '8',
  'elk.spacing.edgeEdge': '8',
  'elk.layered.spacing.edgeNodeBetweenLayers': '24',
  'elk.layered.nodePlacement.strategy': 'NETWORK_SIMPLEX',
  'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
  'elk.layered.thoroughness': '12',
  'elk.separateConnectedComponents': 'true',
  'elk.spacing.componentComponent': '64',
  'elk.padding': '[top=32,left=32,bottom=32,right=32]',
}

/** Card box. The card component fills exactly this, so wires meet its border. */
export const CARD_WIDTH = 256
export const CARD_HEIGHT = 116
/** Neighbour pills in the "Used by" and "Uses" bands. */
export const PILL_HEIGHT = 40
const PILL_MIN_WIDTH = 168
const PILL_MAX_WIDTH = 288
/** Icon tile, gaps, direction icon and padding around a pill's label. */
const PILL_CHROME = 76
/** Vertical room per wire end, so many wires fan out along a side instead of piling up. */
const PER_WIRE = 7
/** Bracket line above the bands, this far over the topmost card. */
const BAND_OFFSET = 30
export const BAND_HEIGHT = 20

export interface Point {
  x: number
  y: number
}

export interface CardBox {
  card: BoardCard
  x: number
  y: number
  width: number
  height: number
  /** Left-to-right entrance order, for the staggered rise. */
  order: number
}

export interface WireRoute {
  wire: BoardWire
  /** ELK's orthogonal route: start, bends, end, in board coordinates. */
  points: Point[]
}

export interface BandBox {
  id: 'used-by' | 'inside' | 'uses'
  label: string
  x: number
  y: number
  width: number
}

export interface BoardLayout {
  /** The board's key: a different key is a different board. */
  key: string
  board: Board
  cards: CardBox[]
  wires: WireRoute[]
  bands: BandBox[]
}

let engine: InstanceType<typeof ELK> | null = null
function elk() {
  engine ??= new ELK()
  return engine
}

let measureContext: CanvasRenderingContext2D | null | undefined
/** Rendered width of a pill label, measured with the real font when a canvas is available. */
function textWidth(text: string): number {
  if (measureContext === undefined) {
    measureContext = typeof document === 'undefined' ? null : document.createElement('canvas').getContext('2d')
    if (measureContext) measureContext.font = '500 13px "Geist Variable", ui-sans-serif, system-ui, sans-serif'
  }
  return measureContext ? measureContext.measureText(text).width : text.length * 7.2
}

const isPill = (card: BoardCard) => card.role === 'used-by' || card.role === 'uses'

function cardSize(card: BoardCard, degree: number) {
  if (isPill(card)) {
    const width = Math.min(PILL_MAX_WIDTH, Math.max(PILL_MIN_WIDTH, Math.ceil(textWidth(card.label)) + PILL_CHROME))
    return { width, height: Math.max(PILL_HEIGHT, degree * PER_WIRE + 14) }
  }
  return { width: CARD_WIDTH, height: Math.max(CARD_HEIGHT, degree * PER_WIRE + 28) }
}

function layerOption(card: BoardCard): LayoutOptions | undefined {
  if (card.role === 'used-by') return { 'elk.layered.layering.layerConstraint': 'FIRST_SEPARATE' }
  if (card.role === 'uses') return { 'elk.layered.layering.layerConstraint': 'LAST_SEPARATE' }
  if (card.layer === 'last') return { 'elk.layered.layering.layerConstraint': 'LAST' }
  return undefined
}

function bandsFor(board: Board, cards: readonly CardBox[]): BandBox[] {
  if (!board.banded || cards.length === 0) return []
  const top = Math.min(...cards.map((box) => box.y))
  const groups: { id: BandBox['id']; label: string; boxes: CardBox[] }[] = [
    { id: 'used-by', label: 'Used by', boxes: cards.filter((box) => box.card.role === 'used-by') },
    {
      id: 'inside',
      label: cards.some((box) => box.card.role === 'member' || box.card.role === 'group') ? `Inside ${board.title}` : board.title,
      boxes: cards.filter((box) => !isPill(box.card)),
    },
    { id: 'uses', label: 'Uses', boxes: cards.filter((box) => box.card.role === 'uses') },
  ]
  return groups
    .filter((group) => group.boxes.length > 0)
    .map((group) => {
      const left = Math.min(...group.boxes.map((box) => box.x))
      const right = Math.max(...group.boxes.map((box) => box.x + box.width))
      return { id: group.id, label: group.label, x: left, y: top - BAND_OFFSET - BAND_HEIGHT / 2, width: right - left }
    })
}

/** Last resort when ELK throws: cards in a grid, wires as straight runs. Never expected, never blank. */
function gridFallback(board: Board): BoardLayout {
  const columns = Math.max(1, Math.ceil(Math.sqrt(board.cards.length)))
  const cards = board.cards.map((card, index) => {
    const { width, height } = cardSize(card, 0)
    return { card, x: (index % columns) * (CARD_WIDTH + 48), y: Math.floor(index / columns) * (CARD_HEIGHT + 48), width, height, order: index }
  })
  const byId = new Map(cards.map((box) => [box.card.id, box]))
  const wires = board.wires.flatMap((wire) => {
    const from = byId.get(wire.source)
    const to = byId.get(wire.target)
    if (!from || !to) return []
    return [{ wire, points: [{ x: from.x + from.width, y: from.y + from.height / 2 }, { x: to.x, y: to.y + to.height / 2 }] }]
  })
  return { key: board.key, board, cards, wires, bands: bandsFor(board, cards) }
}

/** Lays a board out. Pure apart from the shared ELK instance; the caller drops stale results. */
export async function layoutBoard(board: Board): Promise<BoardLayout> {
  const degree = new Map<string, number>()
  for (const wire of board.wires) {
    degree.set(wire.source, (degree.get(wire.source) ?? 0) + 1)
    degree.set(wire.target, (degree.get(wire.target) ?? 0) + 1)
  }

  const children: ElkNode[] = board.cards.map((card) => ({
    id: card.id,
    ...cardSize(card, degree.get(card.id) ?? 0),
    layoutOptions: layerOption(card),
  }))
  const edges: ElkExtendedEdge[] = board.wires.map((wire) => ({ id: wire.id, sources: [wire.source], targets: [wire.target] }))

  let result: ElkNode
  try {
    result = await elk().layout({ id: 'board', layoutOptions: ROOT_OPTIONS, children, edges })
  } catch (error) {
    console.error('Architecture layout failed; showing a grid instead.', error)
    return gridFallback(board)
  }

  const cardById = new Map(board.cards.map((card) => [card.id, card]))
  const placed: Omit<CardBox, 'order'>[] = (result.children ?? []).flatMap((node) => {
    const card = cardById.get(node.id)
    if (!card) return []
    return [{ card, x: node.x ?? 0, y: node.y ?? 0, width: node.width ?? CARD_WIDTH, height: node.height ?? CARD_HEIGHT }]
  })
  // Cards rise in left to right, top to bottom.
  const order = new Map(
    [...placed].sort((a, b) => a.x - b.x || a.y - b.y).map((box, index) => [box.card.id, index]),
  )
  const cards = placed.map((box) => ({ ...box, order: order.get(box.card.id) ?? 0 }))

  const wireById = new Map(board.wires.map((wire) => [wire.id, wire]))
  const wires: WireRoute[] = (result.edges ?? []).flatMap((edge) => {
    const wire = wireById.get(edge.id)
    const section = edge.sections?.[0]
    if (!wire || !section) return []
    return [{ wire, points: [section.startPoint, ...(section.bendPoints ?? []), section.endPoint] }]
  })

  return { key: board.key, board, cards, wires, bands: bandsFor(board, cards) }
}
