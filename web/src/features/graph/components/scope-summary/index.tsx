import { FolderGitIcon, InfoIcon, SearchIcon } from 'lucide-react'
import { useMemo } from 'react'

import { Row, RowList, Stat, StatStrip } from '@/components/system'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'

import { descendantCount, type Board, type BoardCard, type Hierarchy } from '../../lib/board'
import { NODE_KIND_META, SYSTEM_GROUPS } from '../../lib/labels'

interface ScopeSummaryProps {
  board: Board
  hierarchy: Hierarchy
  /** A row was picked: select the card's node (or unfold a group) and bring it into view. */
  onPick: (card: BoardCard) => void
  /** Select a node that has no card here: the codebase the overview stands for. */
  onSelectNode: (nodeId: string) => void
}

interface Section {
  title: string
  hint?: string
  cards: readonly BoardCard[]
}

function sectionsFor(board: Board): Section[] {
  if (board.scope.type === 'focus') {
    return [
      { title: 'Used by', cards: board.cards.filter((card) => card.role === 'used-by') },
      { title: 'Inside', cards: board.cards.filter((card) => card.role === 'member' || card.role === 'self' || card.role === 'group') },
      { title: 'Uses', cards: board.cards.filter((card) => card.role === 'uses') },
    ]
  }
  if (board.scope.type === 'search') return [{ title: 'Matches', cards: board.cards }]

  const placed = new Set<string>()
  const sections: Section[] = SYSTEM_GROUPS.map((group) => {
    const cards = board.cards.filter((card) => group.kinds.includes(card.kind))
    for (const card of cards) placed.add(card.id)
    return { title: group.label, hint: group.hint, cards }
  })
  sections.push({ title: 'Other', cards: board.cards.filter((card) => !placed.has(card.id)) })
  return sections
}

function metaFor(card: BoardCard) {
  if (card.role === 'group') return card.description
  return card.inside || card.description || NODE_KIND_META[card.kind].label
}

/**
 * The panel while nothing is selected: what the current board is, a few
 * numbers, and every card on it as a list, grouped the way Floorplan groups
 * its systems. Picking a row selects the card and brings it into view.
 */
export function ScopeSummary({ board, hierarchy, onPick, onSelectNode }: ScopeSummaryProps) {
  const sections = useMemo(() => sectionsFor(board).filter((section) => section.cards.length > 0), [board])
  const focusNode = board.scope.type === 'focus' ? hierarchy.byId.get(board.scope.nodeId) : undefined
  const Icon = focusNode ? NODE_KIND_META[focusNode.kind].icon : board.scope.type === 'search' ? SearchIcon : FolderGitIcon
  const relations = board.wires.reduce((total, wire) => total + wire.count, 0)

  const stats =
    board.scope.type === 'focus' && focusNode
      ? [
          { label: 'Inside', value: descendantCount(hierarchy, focusNode.id) },
          { label: 'Used by', value: board.cards.filter((card) => card.role === 'used-by').length },
          { label: 'Uses', value: board.cards.filter((card) => card.role === 'uses').length },
        ]
      : [
          { label: board.scope.type === 'search' ? 'Matches' : 'Parts', value: board.cards.length },
          { label: 'Nodes', value: hierarchy.byId.size },
          { label: 'Relations', value: relations },
        ]

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-start gap-2.5">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
          <Icon className="size-4" />
        </span>
        <div className="flex min-w-0 flex-col gap-0.5">
          <h3 className="text-sm leading-5 font-medium text-balance">{board.title}</h3>
          <span className="text-2xs text-muted-foreground">{board.caption ?? (board.scope.type === 'overview' ? 'Codebase' : 'Search')}</span>
        </div>
      </header>

      {board.description ? <p className="text-xs text-muted-foreground">{board.description}</p> : null}

      {board.scope.type === 'overview' && hierarchy.codebaseId !== null ? (
        // The repository's own relations (what documents it, who owns it) and its impact live on its node.
        <Button variant="outline" size="sm" className="self-start" onClick={() => onSelectNode(hierarchy.codebaseId as string)}>
          <InfoIcon data-icon="inline-start" />
          Repository details
        </Button>
      ) : null}

      <StatStrip className="grid-cols-3 sm:grid-cols-3">
        {stats.map((stat) => (
          <Stat key={stat.label} label={stat.label} value={stat.value} />
        ))}
      </StatStrip>

      {sections.map((section) => (
        <section key={section.title} className="flex flex-col gap-1.5" aria-label={section.title}>
          <div className="flex items-baseline gap-2">
            <h4 className="text-xs font-medium">{section.title}</h4>
            <span className="text-2xs tabular-nums text-muted-foreground">{section.cards.length}</span>
            {section.hint ? <span className="truncate text-2xs text-muted-foreground">{section.hint}</span> : null}
          </div>
          <RowList className="-mx-4">
            {section.cards.map((card) => {
              const CardIcon = NODE_KIND_META[card.kind].icon
              return (
                <li key={card.id}>
                  <Row
                    size="md"
                    leading={<CardIcon className="text-muted-foreground" />}
                    title={card.label}
                    meta={<span className="truncate">{metaFor(card)}</span>}
                    onClick={() => onPick(card)}
                    className="px-4"
                  />
                </li>
              )
            })}
          </RowList>
        </section>
      ))}

      <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-2xs text-muted-foreground">
        <span>Double-click a card, or press Enter twice, to open it</span>
        <span aria-hidden>·</span>
        <span className="inline-flex items-center gap-1">
          <Kbd>⌫</Kbd> up
        </span>
        <span className="inline-flex items-center gap-1">
          <Kbd>F</Kbd> fit
        </span>
        <span className="inline-flex items-center gap-1">
          <Kbd>/</Kbd> search
        </span>
      </p>
    </div>
  )
}
