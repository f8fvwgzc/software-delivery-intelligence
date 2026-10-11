import type { DiagramEdge, DiagramNode, DiagramSpec } from '../../../api/queries'
import { EDGE_STYLE, edgeStyle, type EdgeStyle } from '../../../lib/labels'
import { edgeKey } from '../keys'
import { groupTints, nodeTint, type Tint } from '../palette'
import { fitText, textWidth, wrapText, type Font } from '../text'
import type { Box } from '../types'

export const PARTICIPANT_FONT: Font = { size: 13, weight: 500 }
export const CAPTION_FONT: Font = { size: 11, mono: true }
export const MESSAGE_FONT: Font = { size: 12 }

/** Each participant owns a 180 px column; its box is 150 px wide. */
export const SLOT_WIDTH = 180
export const PARTICIPANT_WIDTH = 150
export const PARTICIPANT_HEIGHT = 44
/** Room above the box for an actor's head. */
export const HEAD_HEIGHT = 24
export const HEAD_RADIUS = 9
/** Vertical distance between messages. */
export const STEP = 36
const TOP_GAP = 28
const BOTTOM_GAP = 24
/** How far a self-message loops out to the right. */
export const SELF_WIDTH = 32
export const ACTIVATION_WIDTH = 8

export interface Participant {
  node: DiagramNode
  index: number
  /** Lifeline x. */
  x: number
  box: Box
  tint: Tint
  lines: string[]
  caption: string
  actor: boolean
}

export interface Message {
  edge: DiagramEdge
  id: string
  style: EdgeStyle
  /** Dashed with an open head: a return. Everything else is a call with a filled head. */
  returns: boolean
  label: string
  y: number
  from: Participant
  to: Participant
  self: boolean
}

export interface Activation {
  participant: Participant
  from: number
  to: number
}

export interface SequenceLayout {
  participants: Participant[]
  messages: Message[]
  activations: Activation[]
  /** Where lifelines start and end. */
  lifeline: { top: number; bottom: number }
  hasCalls: boolean
  hasReturns: boolean
  content: Box
}

/** Edges go in the order of the number in their id (m01, m02 … or e1, e2 …), array order when there is none. */
function messageOrder(edges: readonly DiagramEdge[]): number[] {
  const order = edges.map((_, index) => index)
  const rank = (index: number) => {
    const match = edges[index].id?.match(/(\d+)\s*$/)
    return match ? Number(match[1]) : Number.POSITIVE_INFINITY
  }
  return order.sort((a, b) => {
    const diff = rank(a) - rank(b)
    return Number.isNaN(diff) || diff === 0 ? a - b : diff
  })
}

/**
 * Participants across the top in spec order, messages top-down in id
 * order at 36 px steps, an activation on the callee from a call to the
 * return that answers it.
 */
export function layoutSequence(spec: DiagramSpec): SequenceLayout {
  const nodes = spec.nodes ?? []
  const edges = spec.edges ?? []
  const tints = groupTints(spec.groups ?? [])

  const participants: Participant[] = nodes.map((node, index) => {
    const x = index * SLOT_WIDTH + SLOT_WIDTH / 2
    const inner = PARTICIPANT_WIDTH - 20
    const caption = node.caption?.trim() ?? ''
    return {
      node,
      index,
      x,
      box: { x: x - PARTICIPANT_WIDTH / 2, y: HEAD_HEIGHT, w: PARTICIPANT_WIDTH, h: PARTICIPANT_HEIGHT },
      tint: nodeTint(node, tints),
      lines: wrapText(node.label, inner, PARTICIPANT_FONT, caption ? 1 : 2),
      caption: caption ? fitText(caption, inner, CAPTION_FONT) : '',
      actor: node.kind === 'actor',
    }
  })
  const byId = new Map(participants.map((participant) => [participant.node.id, participant]))

  const top = HEAD_HEIGHT + PARTICIPANT_HEIGHT
  const messages: Message[] = []
  let hasCalls = false
  let hasReturns = false
  for (const index of messageOrder(edges)) {
    const edge = edges[index]
    const from = byId.get(edge.source)
    const to = byId.get(edge.target)
    if (!from || !to) continue
    const style = edgeStyle(edge.style)
    const returns = style === 'returns' || EDGE_STYLE[style].dashed
    if (returns) hasReturns = true
    else hasCalls = true
    messages.push({
      edge,
      id: edgeKey(edge, index),
      style,
      returns,
      label: edge.label?.trim() ?? '',
      y: top + TOP_GAP + messages.length * STEP + STEP / 2,
      from,
      to,
      self: from === to,
    })
  }

  // A call is answered by the first later return going the other way that no earlier call has claimed.
  const activations: Activation[] = []
  const claimed = new Set<number>()
  messages.forEach((call, index) => {
    if (call.returns || call.self) return
    for (let later = index + 1; later < messages.length; later++) {
      const candidate = messages[later]
      if (claimed.has(later) || !candidate.returns) continue
      if (candidate.from === call.to && candidate.to === call.from) {
        claimed.add(later)
        activations.push({ participant: call.to, from: call.y, to: candidate.y })
        break
      }
    }
  })

  const bottom = top + TOP_GAP + Math.max(messages.length, 1) * STEP + BOTTOM_GAP
  // A self-message's label hangs to the right of its loop and may pass the last column.
  const width = messages.reduce(
    (max, message) =>
      message.self ? Math.max(max, message.from.x + SELF_WIDTH + 8 + textWidth(message.label, MESSAGE_FONT)) : max,
    Math.max(participants.length, 1) * SLOT_WIDTH,
  )
  return {
    participants,
    messages,
    activations,
    lifeline: { top, bottom: bottom - BOTTOM_GAP / 2 },
    hasCalls,
    hasReturns,
    content: { x: 0, y: 0, w: width, h: bottom },
  }
}
