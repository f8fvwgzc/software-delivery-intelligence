import { NODE_KINDS, NODE_KIND_META } from './labels'
import type { Graph, GraphEdge, GraphNode, NodeKind, RelationCategory } from './types'

/**
 * The board model behind the architecture map, after Floorplan
 * (github.com/haydenschmitty/floorplan, MIT): containment is nesting, not a
 * wire. The overview shows the top-level parts of the codebase as cards; a
 * card that holds other nodes opens into a focus board laid out as
 * "Used by | Inside | Uses". Every other relation is a wire, lifted from
 * whatever node it starts or ends on to the card that stands for that node
 * on the current board, so a board never shows a node twice and never drops
 * a relation of something folded inside a card.
 *
 * Pure: no React, no layout. The canvas lays a `Board` out with ELK.
 */

/** The structural relation that nests one node inside another. */
export const CONTAINS = 'CONTAINS'

/** Kinds drawn in the last layer of the overview, like Floorplan's outside services. */
const OUTSIDE_KINDS: ReadonlySet<NodeKind> = new Set(['datastore', 'external_api'])

/** Unwired siblings of one kind fold into a single group card from this many on. */
const GROUP_MIN = 4

export interface Hierarchy {
  byId: ReadonlyMap<string, GraphNode>
  byKey: ReadonlyMap<string, GraphNode>
  /** Nearest visible container of a visible node. */
  parentOf: ReadonlyMap<string, string>
  /** Visible children in display order (kind order, then label). */
  childrenOf: ReadonlyMap<string, readonly string[]>
  /** Visible nodes without a visible container, in display order. */
  roots: readonly string[]
  /** Every edge between visible nodes that is not current containment. */
  wires: readonly GraphEdge[]
  /** The single repository the overview stands for, when there is one. */
  codebaseId: string | null
}

const kindRank = new Map(NODE_KINDS.map((kind, index) => [kind, index]))

function byDisplayOrder(a: GraphNode, b: GraphNode) {
  return (kindRank.get(a.kind) ?? 99) - (kindRank.get(b.kind) ?? 99) || a.label.localeCompare(b.label)
}

function isNesting(edge: GraphEdge) {
  return edge.relation === CONTAINS && edge.validTo == null
}

/**
 * Containment from current `CONTAINS` edges among the visible nodes. A node
 * whose container is hidden (its kind is filtered out) moves up to the
 * nearest visible ancestor; a second container or a cycle is ignored, so the
 * result is always a forest.
 */
export function buildHierarchy(graph: Graph, visibleKinds: ReadonlySet<NodeKind>): Hierarchy {
  const allById = new Map(graph.nodes.map((node) => [node.id, node]))
  const rawParent = new Map<string, string>()

  for (const edge of graph.edges) {
    if (!isNesting(edge)) continue
    const { sourceId: parent, targetId: child } = edge
    if (parent === child || rawParent.has(child) || !allById.has(parent) || !allById.has(child)) continue
    // Refuse an edge that would close a cycle: `child` must not already be an ancestor of `parent`.
    let cursor: string | undefined = parent
    let cycle = false
    while (cursor !== undefined) {
      if (cursor === child) {
        cycle = true
        break
      }
      cursor = rawParent.get(cursor)
    }
    if (!cycle) rawParent.set(child, parent)
  }

  const visible = graph.nodes.filter((node) => visibleKinds.has(node.kind))
  const byId = new Map(visible.map((node) => [node.id, node]))
  const byKey = new Map(visible.map((node) => [node.key, node]))

  const parentOf = new Map<string, string>()
  for (const node of visible) {
    let cursor = rawParent.get(node.id)
    while (cursor !== undefined && !byId.has(cursor)) cursor = rawParent.get(cursor)
    if (cursor !== undefined) parentOf.set(node.id, cursor)
  }

  const childLists = new Map<string, GraphNode[]>()
  const rootNodes: GraphNode[] = []
  for (const node of visible) {
    const parent = parentOf.get(node.id)
    if (parent === undefined) {
      rootNodes.push(node)
      continue
    }
    const list = childLists.get(parent)
    if (list) list.push(node)
    else childLists.set(parent, [node])
  }

  const childrenOf = new Map<string, readonly string[]>()
  for (const [parent, list] of childLists) childrenOf.set(parent, list.sort(byDisplayOrder).map((node) => node.id))
  rootNodes.sort(byDisplayOrder)

  const wires = graph.edges.filter(
    (edge) => !(isNesting(edge) && parentOf.get(edge.targetId) === edge.sourceId) && byId.has(edge.sourceId) && byId.has(edge.targetId),
  )

  const repositories = rootNodes.filter((node) => node.kind === 'repository' && childrenOf.has(node.id))
  const codebaseId = repositories.length === 1 ? repositories[0].id : null

  return { byId, byKey, parentOf, childrenOf, roots: rootNodes.map((node) => node.id), wires, codebaseId }
}

/** Every visible node under `id`, not counting `id` itself. */
export function descendantCount(hierarchy: Hierarchy, id: string): number {
  let total = 0
  const stack = [...(hierarchy.childrenOf.get(id) ?? [])]
  while (stack.length > 0) {
    const next = stack.pop() as string
    total++
    stack.push(...(hierarchy.childrenOf.get(next) ?? []))
  }
  return total
}

/** `id` and every container above it, nearest first. */
export function ancestry(hierarchy: Hierarchy, id: string): string[] {
  const chain: string[] = []
  let cursor: string | undefined = id
  while (cursor !== undefined) {
    chain.push(cursor)
    cursor = hierarchy.parentOf.get(cursor)
  }
  return chain
}

export type BoardScope = { type: 'overview' } | { type: 'focus'; nodeId: string } | { type: 'search'; query: string }

/**
 * `member` a part of the board; `self` the focused node itself, when it has
 * wires of its own or nothing inside; `used-by` / `uses` its neighbours
 * outside, in the first and last band; `group` unwired siblings of one kind.
 */
export type CardRole = 'member' | 'self' | 'used-by' | 'uses' | 'group'

export interface BoardCard {
  /** Unique on the board: role and node, or the group's kind. */
  id: string
  role: CardRole
  /** The node this card is; null for a group. */
  node: GraphNode | null
  kind: NodeKind
  label: string
  description: string
  /** Short facts from the node's attributes, e.g. `cargo`, `axum, tonic`. */
  chips: readonly string[]
  /** What is folded inside, e.g. "1 migration", "3 parts"; empty when nothing. */
  inside: string
  /** Visible nodes inside it: the card opens into a focus board when above zero. */
  childCount: number
  /** Every node id this card stands for: itself, everything inside, or a group's members. */
  members: readonly string[]
  /** Containing node's label, for neighbours that live inside something else. */
  context: string | null
  /** Layer pin for the layout. */
  layer: 'first' | 'last' | null
}

export interface BoardWire {
  id: string
  source: string
  target: string
  /** Distinct verbs carried, e.g. `DEPENDS_ON`. */
  relations: readonly string[]
  category: RelationCategory
  /** Underlying edges folded into this wire. */
  count: number
  /** Every underlying relation has stopped holding. */
  expired: boolean
}

export interface Crumb {
  /** Null for the overview. */
  nodeId: string | null
  label: string
}

export interface Board {
  scope: BoardScope
  /** Changes whenever a different board is shown; restarts the entrance animation. */
  key: string
  title: string
  /** Kind caption under the title; null for the overview and search. */
  caption: string | null
  description: string
  cards: readonly BoardCard[]
  wires: readonly BoardWire[]
  trail: readonly Crumb[]
  /** A focus board: draw the "Used by / Inside / Uses" bands. */
  banded: boolean
}

function plural(kind: NodeKind, count: number) {
  const meta = NODE_KIND_META[kind]
  return `${count} ${count === 1 ? meta.label.toLowerCase() : meta.plural}`
}

const CHIP_ATTRIBUTES = ['frameworks', 'framework', 'ecosystem', 'engine', 'tool', 'provider'] as const

/** Up to two short facts: what it is built with, then how much of it there is. */
export function cardChips(node: GraphNode): string[] {
  const chips: string[] = []
  for (const name of CHIP_ATTRIBUTES) {
    const value = node.attributes[name]
    if (value && !chips.includes(value)) chips.push(value)
    if (chips.length === 1) break
  }
  const { jobs, count, stages } = node.attributes
  if (jobs) chips.push(`${jobs} ${jobs === '1' ? 'job' : 'jobs'}`)
  else if (count) chips.push(`${count} files`)
  else if (stages) chips.push(`${stages} stages`)
  return chips.slice(0, 2)
}

function insideSummary(hierarchy: Hierarchy, id: string): string {
  const children = hierarchy.childrenOf.get(id) ?? []
  if (children.length === 0) return ''
  const kinds = new Set(children.map((child) => hierarchy.byId.get(child)?.kind))
  const total = descendantCount(hierarchy, id)
  if (kinds.size === 1 && total === children.length) {
    const kind = hierarchy.byId.get(children[0])?.kind ?? 'unspecified'
    return plural(kind, total)
  }
  return `${total} inside`
}

function subtree(hierarchy: Hierarchy, id: string): string[] {
  const ids = [id]
  for (let index = 0; index < ids.length; index++) ids.push(...(hierarchy.childrenOf.get(ids[index]) ?? []))
  return ids
}

function nodeCard(hierarchy: Hierarchy, node: GraphNode, role: CardRole): BoardCard {
  const parentId = hierarchy.parentOf.get(node.id)
  const parent = parentId !== undefined && parentId !== hierarchy.codebaseId ? hierarchy.byId.get(parentId) : undefined
  const folded = role === 'member' || role === 'self'
  return {
    id: `${role}:${node.id}`,
    role,
    node,
    kind: node.kind,
    label: node.label,
    description: node.description,
    chips: cardChips(node),
    // The focused node's own card stands only for itself: its board is the one already open.
    inside: role === 'self' ? '' : insideSummary(hierarchy, node.id),
    childCount: role === 'self' ? 0 : descendantCount(hierarchy, node.id),
    // A member stands for its whole subtree; the focused node's own card only for itself.
    members: folded && role !== 'self' ? subtree(hierarchy, node.id) : [node.id],
    context: role === 'used-by' || role === 'uses' ? (parent?.label ?? null) : null,
    layer: role === 'used-by' ? 'first' : role === 'uses' ? 'last' : null,
  }
}

/**
 * A wire folding relations of several families is drawn in the style of the
 * most telling one, the same whatever order the edges arrive in: governance,
 * then validation, then infrastructure, then the plain families.
 */
const CATEGORY_RANK: Record<RelationCategory, number> = {
  governance: 0,
  validation: 1,
  infrastructure: 2,
  execution: 3,
  source: 4,
  outcome: 5,
  ownership: 6,
  structure: 7,
  unspecified: 8,
}

interface WireDraft {
  source: string
  target: string
  relations: Set<string>
  category: RelationCategory
  count: number
  expired: boolean
}

/** Folds edges onto card pairs. `resolve` maps an edge end to its card id, or null when it is off the board. */
function liftWires(edges: readonly GraphEdge[], resolve: (nodeId: string, edge: GraphEdge, end: 'source' | 'target') => string | null) {
  const drafts = new Map<string, WireDraft>()
  for (const edge of edges) {
    const source = resolve(edge.sourceId, edge, 'source')
    const target = resolve(edge.targetId, edge, 'target')
    if (source === null || target === null || source === target) continue
    const key = `${source}->${target}`
    const draft = drafts.get(key)
    const expired = edge.validTo != null
    if (draft) {
      draft.relations.add(edge.relation)
      draft.count++
      draft.expired &&= expired
      if (CATEGORY_RANK[edge.category] < CATEGORY_RANK[draft.category]) draft.category = edge.category
    } else {
      drafts.set(key, { source, target, relations: new Set([edge.relation]), category: edge.category, count: 1, expired })
    }
  }
  return [...drafts.entries()].map<BoardWire>(([id, draft]) => ({
    id,
    source: draft.source,
    target: draft.target,
    relations: [...draft.relations],
    category: draft.category,
    count: draft.count,
    expired: draft.expired,
  }))
}

/** Maps every node id to the card standing for it, through `members`. */
function ownerIndex(cards: readonly BoardCard[]) {
  const owner = new Map<string, string>()
  for (const card of cards) for (const id of card.members) if (!owner.has(id)) owner.set(id, card.id)
  return owner
}

/**
 * Folds unwired member cards of one kind into a group card once there are
 * `GROUP_MIN` of them, unless that kind is expanded. Wires are recomputed by
 * the caller afterwards, so a group never hides a relation.
 */
function foldGroups(cards: BoardCard[], wires: readonly BoardWire[], expanded: ReadonlySet<NodeKind>): BoardCard[] {
  const wired = new Set<string>()
  for (const wire of wires) {
    wired.add(wire.source)
    wired.add(wire.target)
  }
  const loose = new Map<NodeKind, BoardCard[]>()
  for (const card of cards) {
    if (card.role !== 'member' || wired.has(card.id) || expanded.has(card.kind)) continue
    const list = loose.get(card.kind)
    if (list) list.push(card)
    else loose.set(card.kind, [card])
  }

  const folded = new Set<string>()
  const groups: BoardCard[] = []
  for (const [kind, list] of loose) {
    if (list.length < GROUP_MIN) continue
    for (const card of list) folded.add(card.id)
    const whole = cards.filter((card) => card.role === 'member' && card.kind === kind).length === list.length
    const names = list.slice(0, 3).map((card) => card.label)
    const rest = list.length - names.length
    groups.push({
      id: `group:${kind}`,
      role: 'group',
      node: null,
      kind,
      label: `${whole ? '' : 'Other '}${NODE_KIND_META[kind].plural}`.replace(/^./, (first) => first.toUpperCase()),
      description: rest > 0 ? `${names.join(', ')} and ${rest} more` : names.join(', '),
      chips: [],
      inside: plural(kind, list.length),
      childCount: list.length,
      members: list.flatMap((card) => card.members),
      context: null,
      layer: null,
    })
  }
  if (groups.length === 0) return cards
  return [...cards.filter((card) => !folded.has(card.id)), ...groups]
}

function crumbs(hierarchy: Hierarchy, nodeId: string): Crumb[] {
  const chain = ancestry(hierarchy, nodeId)
    .filter((id) => id !== hierarchy.codebaseId)
    .reverse()
  return [
    { nodeId: null, label: 'Overview' },
    ...chain.map((id) => ({ nodeId: id, label: hierarchy.byId.get(id)?.label ?? id })),
  ]
}

function overviewBoard(hierarchy: Hierarchy, expanded: ReadonlySet<NodeKind>): Board {
  const codebase = hierarchy.codebaseId === null ? null : hierarchy.byId.get(hierarchy.codebaseId)
  const memberIds = codebase
    ? [...(hierarchy.childrenOf.get(codebase.id) ?? []), ...hierarchy.roots.filter((id) => id !== codebase.id)]
    : hierarchy.roots
  const members = memberIds
    .map((id) => hierarchy.byId.get(id))
    .filter((node): node is GraphNode => node !== undefined)
    .map((node) => {
      const card = nodeCard(hierarchy, node, 'member')
      return OUTSIDE_KINDS.has(node.kind) ? { ...card, layer: 'last' as const } : card
    })

  const lift = (cards: readonly BoardCard[]) => {
    const owner = ownerIndex(cards)
    return liftWires(hierarchy.wires, (id) => owner.get(id) ?? null)
  }
  const cards = foldGroups(members, lift(members), expanded)

  return {
    scope: { type: 'overview' },
    key: 'overview',
    title: codebase?.label ?? 'Overview',
    caption: null,
    description: codebase?.description ?? 'Every part the scan found and how the parts connect.',
    cards,
    wires: lift(cards),
    trail: [{ nodeId: null, label: 'Overview' }],
    banded: false,
  }
}

function focusBoard(hierarchy: Hierarchy, node: GraphNode, expanded: ReadonlySet<NodeKind>): Board {
  const inScope = new Set(subtree(hierarchy, node.id))
  const childIds = hierarchy.childrenOf.get(node.id) ?? []
  const ownWires = hierarchy.wires.some((edge) => edge.sourceId === node.id || edge.targetId === node.id)

  let inside: BoardCard[] = childIds
    .map((id) => hierarchy.byId.get(id))
    .filter((child): child is GraphNode => child !== undefined)
    .map((child) => nodeCard(hierarchy, child, 'member'))
  const self = childIds.length === 0 || ownWires ? [nodeCard(hierarchy, node, 'self')] : []

  const neighbours = new Map<string, BoardCard>()
  const neighbour = (id: string, role: 'used-by' | 'uses') => {
    const cardId = `${role}:${id}`
    if (!neighbours.has(cardId)) {
      const other = hierarchy.byId.get(id)
      if (!other) return null
      neighbours.set(cardId, nodeCard(hierarchy, other, role))
    }
    return cardId
  }

  const lift = (insideCards: readonly BoardCard[]) => {
    const owner = ownerIndex([...self, ...insideCards])
    return liftWires(hierarchy.wires, (id, edge, end) => {
      const own = owner.get(id)
      if (own !== undefined) return own
      if (inScope.has(id)) return null
      // Outside the focused subtree: a neighbour only when the other end is inside it.
      const other = end === 'source' ? edge.targetId : edge.sourceId
      if (!inScope.has(other)) return null
      return neighbour(id, end === 'source' ? 'used-by' : 'uses')
    })
  }

  inside = foldGroups(inside, lift(inside), expanded)
  const wires = lift(inside)
  const crumbTrail = crumbs(hierarchy, node.id)

  return {
    scope: { type: 'focus', nodeId: node.id },
    key: `focus:${node.id}`,
    title: node.label,
    caption: NODE_KIND_META[node.kind].label,
    description: node.description,
    cards: [...neighbours.values()].filter((card) => card.role === 'used-by').concat(self, inside, [...neighbours.values()].filter((card) => card.role === 'uses')),
    wires,
    trail: crumbTrail,
    banded: true,
  }
}

function searchBoard(hierarchy: Hierarchy, query: string): Board {
  const needle = query.trim().toLowerCase()
  const matches = [...hierarchy.byId.values()]
    .filter((node) => node.label.toLowerCase().includes(needle) || node.key.toLowerCase().includes(needle))
    .sort(byDisplayOrder)
  const cards = matches.map((node) => ({ ...nodeCard(hierarchy, node, 'member'), members: [node.id] }))
  const owner = ownerIndex(cards)
  return {
    scope: { type: 'search', query },
    key: `search:${needle}`,
    title: `Results for “${query.trim()}”`,
    caption: null,
    description: 'Matching nodes and the relations between them. Open one to see it in place.',
    cards,
    wires: liftWires(hierarchy.wires, (id) => owner.get(id) ?? null),
    trail: [{ nodeId: null, label: 'Overview' }],
    banded: false,
  }
}

/** The board for a scope. A focus on the codebase repository itself is the overview. */
export function buildBoard(hierarchy: Hierarchy, scope: BoardScope, expanded: ReadonlySet<NodeKind> = new Set()): Board {
  if (scope.type === 'search' && scope.query.trim() !== '') return searchBoard(hierarchy, scope.query)
  if (scope.type === 'focus' && scope.nodeId !== hierarchy.codebaseId) {
    const node = hierarchy.byId.get(scope.nodeId)
    if (node) return focusBoard(hierarchy, node, expanded)
  }
  return overviewBoard(hierarchy, expanded)
}

export interface Connection {
  /** The neighbour, rolled up to the level of the node it connects to. */
  nodeId: string
  relations: readonly string[]
  count: number
}

/**
 * What a node is used by and what it uses, counting every relation with
 * exactly one end inside it (itself or anything it contains). The far end
 * is rolled up to its peer: the ancestor just below the first container it
 * shares with the node, so the list reads at the node's own level rather
 * than as a list of leaves. Strongest first.
 */
export function connections(hierarchy: Hierarchy, nodeId: string): { usedBy: Connection[]; uses: Connection[] } {
  const inside = new Set(subtree(hierarchy, nodeId))
  // The codebase the overview stands for is everyone's container: a part of it
  // rolls up to its top-level part, never to the repository itself.
  const own = new Set(ancestry(hierarchy, nodeId))
  if (hierarchy.codebaseId !== null) own.add(hierarchy.codebaseId)
  const peerOf = (id: string) => {
    const chain = ancestry(hierarchy, id)
    const shared = chain.findIndex((ancestor) => own.has(ancestor))
    if (shared === 0) return chain[0]
    return shared > 0 ? chain[shared - 1] : chain[chain.length - 1]
  }

  const tally = (map: Map<string, { relations: Set<string>; count: number }>, id: string, relation: string) => {
    const entry = map.get(id)
    if (entry) {
      entry.relations.add(relation)
      entry.count++
    } else {
      map.set(id, { relations: new Set([relation]), count: 1 })
    }
  }

  const usedBy = new Map<string, { relations: Set<string>; count: number }>()
  const uses = new Map<string, { relations: Set<string>; count: number }>()
  for (const edge of hierarchy.wires) {
    const fromInside = inside.has(edge.sourceId)
    const toInside = inside.has(edge.targetId)
    if (fromInside === toInside) continue
    if (fromInside) tally(uses, peerOf(edge.targetId), edge.relation)
    else tally(usedBy, peerOf(edge.sourceId), edge.relation)
  }

  const sorted = (map: Map<string, { relations: Set<string>; count: number }>) =>
    [...map.entries()]
      .map(([id, entry]) => ({ nodeId: id, relations: [...entry.relations], count: entry.count }))
      .sort(
        (a, b) =>
          b.count - a.count ||
          (hierarchy.byId.get(a.nodeId)?.label ?? '').localeCompare(hierarchy.byId.get(b.nodeId)?.label ?? ''),
      )
  return { usedBy: sorted(usedBy), uses: sorted(uses) }
}

/** Where a node sits: the board that shows it as a card (its container's), or the overview. */
export function homeScope(hierarchy: Hierarchy, nodeId: string): BoardScope {
  const parent = hierarchy.parentOf.get(nodeId)
  return parent === undefined || parent === hierarchy.codebaseId ? { type: 'overview' } : { type: 'focus', nodeId: parent }
}

/** Card ids joined to a card by a wire, including itself. */
export function neighbourhood(board: Board, cardId: string): Set<string> {
  const ids = new Set([cardId])
  for (const wire of board.wires) {
    if (wire.source === cardId) ids.add(wire.target)
    if (wire.target === cardId) ids.add(wire.source)
  }
  return ids
}
