import { Handle, Position, type NodeProps } from '@xyflow/react'
import { ArrowRightToLineIcon, ArrowUpRightIcon, Maximize2Icon, ChevronsUpDownIcon } from 'lucide-react'
import { createContext, memo, use, type CSSProperties, type MouseEvent } from 'react'

import { Tag } from '@/components/system'
import { cn } from '@/lib/utils'

import type { BoardCard } from '../../lib/board'
import { NODE_KIND_META } from '../../lib/labels'
import type { CardEmphasis, CardFlowNode, PortDot } from './types'

/** Card actions reach the nodes through context, so node data stays plain and memo comparisons stay cheap. */
export const CardActionsContext = createContext<{ open: (card: BoardCard) => void }>({ open: () => {} })

const EMPHASIS_CLASS: Record<CardEmphasis, string> = {
  none: '',
  selected: 'ring-2 ring-brand',
  related: '',
  impacted: 'bg-brand/5 ring-brand/50',
  dimmed: 'arch-dim',
}

function Ports({ ports }: { ports: readonly PortDot[] }) {
  return ports.map((port) => (
    <span
      key={port.id}
      aria-hidden
      data-end={port.end}
      data-state={port.state}
      className="arch-port"
      style={{ left: port.x, top: port.y }}
    />
  ))
}

function stop(event: MouseEvent) {
  event.stopPropagation()
}

/** Opens a drillable card: the visible twin of double-click and Enter. */
function OpenButton({ card }: { card: BoardCard }) {
  const { open } = use(CardActionsContext)
  const group = card.role === 'group'
  const Icon = group ? ChevronsUpDownIcon : Maximize2Icon
  return (
    <button
      type="button"
      className="nodrag nopan arch-open flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      aria-label={group ? `Show all ${card.label.toLowerCase()}` : `Open ${card.label}`}
      title={group ? 'Show each one' : 'Open (double-click)'}
      onClick={(event) => {
        event.stopPropagation()
        open(card)
      }}
      onDoubleClick={stop}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <Icon className="size-3.5" />
    </button>
  )
}

function captionFor(card: BoardCard) {
  const kind = NODE_KIND_META[card.kind].label
  if (card.role === 'self') return `This ${kind.toLowerCase()}`
  if (card.role === 'group') return card.inside
  return kind
}

/**
 * A part on the board, after Floorplan's cards: kind tile and title, the
 * description clamped to two lines, and a footer with what it is built with
 * and what is folded inside. Group cards stack, to read as a collection.
 */
function CardNodeView({ data, selected }: NodeProps<CardFlowNode>) {
  const { card, emphasis, fresh, ports, delay, flash } = data
  const Icon = NODE_KIND_META[card.kind].icon
  const drillable = card.childCount > 0
  const emphasisClass = EMPHASIS_CLASS[selected ? 'selected' : emphasis]

  return (
    <>
      <Handle type="target" position={Position.Left} isConnectable={false} />
      <div className="arch-enter size-full" style={{ '--arch-delay': `${delay}ms` } as CSSProperties}>
        {card.role === 'group' ? (
          <span aria-hidden className="absolute inset-x-2 -bottom-1 h-full rounded-lg bg-card ring-1 ring-border" />
        ) : null}
        <div
          key={flash}
          className={cn(
            'arch-card group/card relative flex size-full flex-col gap-1.5 rounded-lg bg-card p-3 ring-1 ring-border',
            card.role === 'self' && 'ring-foreground/20',
            emphasisClass,
            flash > 0 && 'arch-flash',
          )}
        >
          <div className="flex items-start gap-2.5">
            <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
              <Icon className="size-3.5" />
            </span>
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="arch-title truncate text-[13px] font-medium leading-5 text-foreground">{card.label}</span>
              <span className="arch-caption truncate text-2xs leading-4 text-muted-foreground">{captionFor(card)}</span>
            </div>
            {drillable ? <OpenButton card={card} /> : null}
          </div>
          <p className="arch-desc line-clamp-2 text-xs leading-4 text-muted-foreground">
            {card.description || 'No description yet'}
          </p>
          <div className="arch-meta mt-auto flex min-w-0 items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-1">
              {card.chips.map((chip) => (
                <Tag key={chip} variant="outline" className="min-w-0 truncate">
                  {chip}
                </Tag>
              ))}
            </div>
            <div className="flex shrink-0 items-center gap-1">
              {fresh ? (
                <Tag tone="success" dot>
                  New
                </Tag>
              ) : null}
              {card.inside && card.role !== 'group' ? <Tag variant="outline">{card.inside}</Tag> : null}
            </div>
          </div>
        </div>
      </div>
      <Ports ports={ports} />
      <Handle type="source" position={Position.Right} isConnectable={false} />
    </>
  )
}

/**
 * A neighbour outside the focused node, in the "Used by" or "Uses" band:
 * one row with the kind tile, the name and a direction glyph. Opening it
 * shows it in its own place on the map.
 */
function PillNodeView({ data, selected }: NodeProps<CardFlowNode>) {
  const { card, emphasis, ports, delay, flash } = data
  const Icon = NODE_KIND_META[card.kind].icon
  const Direction = card.role === 'used-by' ? ArrowRightToLineIcon : ArrowUpRightIcon

  return (
    <>
      <Handle type="target" position={Position.Left} isConnectable={false} />
      <div className="arch-enter size-full" style={{ '--arch-delay': `${delay}ms` } as CSSProperties}>
        <div
          key={flash}
          className={cn(
            'arch-card flex size-full items-center gap-2 rounded-lg bg-muted/50 pr-2.5 pl-2 ring-1 ring-border',
            EMPHASIS_CLASS[selected ? 'selected' : emphasis],
            flash > 0 && 'arch-flash',
          )}
        >
          <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-background text-muted-foreground">
            <Icon className="size-3.5" />
          </span>
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="arch-title truncate text-[13px] font-medium leading-4 text-foreground">{card.label}</span>
            {card.context ? (
              <span className="arch-caption truncate text-2xs leading-3.5 text-muted-foreground">in {card.context}</span>
            ) : null}
          </span>
          <Direction aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        </div>
      </div>
      <Ports ports={ports} />
      <Handle type="source" position={Position.Right} isConnectable={false} />
    </>
  )
}

export const CardNode = memo(CardNodeView)
export const PillNode = memo(PillNodeView)
