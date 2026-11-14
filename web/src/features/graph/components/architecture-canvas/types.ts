import type { Edge, Node } from '@xyflow/react'

import type { Board, BoardCard } from '../../lib/board'
import type { ImpactHighlight, RelationCategory } from '../../lib/types'
import type { Point } from './elk-layout'

/**
 * How a card is drawn against the current selection and impact analysis.
 * `related`: joined to the selected card, or reached by its impact analysis.
 */
export type CardEmphasis = 'none' | 'selected' | 'related' | 'impacted' | 'dimmed'

/**
 * A wire's state: `active` (its card or itself is hovered) is drawn strong
 * with the moving flow dash, `linked` (it touches the selection) strong and
 * still, `dimmed` recedes behind a selection.
 */
export type WireState = 'none' | 'active' | 'linked' | 'dimmed'

/** Where a wire meets a card, relative to the card's top-left corner. */
export interface PortDot {
  id: string
  x: number
  y: number
  /** The target end is filled, the source end hollow: direction without arrowheads. */
  end: 'source' | 'target'
  state: WireState
}

export interface CardNodeData extends Record<string, unknown> {
  card: BoardCard
  emphasis: CardEmphasis
  /** Joined the graph since the map was opened (a scan finished meanwhile). */
  fresh: boolean
  ports: readonly PortDot[]
  /** Entrance delay in ms: cards rise in left to right. */
  delay: number
  /** Bumped to replay the locate flash after a reveal. */
  flash: number
}

export interface BandNodeData extends Record<string, unknown> {
  label: string
}

export interface WireEdgeData extends Record<string, unknown> {
  points: readonly Point[]
  relations: readonly string[]
  category: RelationCategory
  count: number
  expired: boolean
  state: WireState
}

export type CardFlowNode = Node<CardNodeData, 'card' | 'pill'>
export type BandFlowNode = Node<BandNodeData, 'band'>
export type ArchNode = CardFlowNode | BandFlowNode
export type ArchEdge = Edge<WireEdgeData, 'wire'>

/** Imperative surface the toolbar, the panel and the page use. */
export interface CanvasHandle {
  fitView: () => void
  /** Pans and zooms to the card standing for a node, and flashes it. */
  focusNode: (nodeId: string) => void
}

/** A one-shot request to locate a node once its board is on screen. */
export interface FocusRequest {
  nodeId: string
  /** The board the node is located on; the request waits until that board is on screen. */
  boardKey: string
  nonce: number
}

export interface ArchitectureCanvasProps {
  board: Board
  /** Selected node id (not card id): selection survives moving between boards. */
  selectedId: string | null
  highlight: ImpactHighlight | null
  /** Node ids that appeared since the map was opened. */
  freshIds: ReadonlySet<string>
  focusRequest: FocusRequest | null
  /** Space the board header takes at the top of the canvas, so fitting never hides cards under it. */
  topInset: number
  onSelect: (nodeId: string | null) => void
  /** Open a card: drill into it, reveal a neighbour in place, or unfold a group. */
  onOpen: (card: BoardCard) => void
  /** Up one level (Backspace, or Escape with nothing selected). */
  onUp: () => void
  /** The focus request was carried out; the page drops it so a remount never replays it. */
  onFocusHandled: (nonce: number) => void
  ref?: React.Ref<CanvasHandle>
}
