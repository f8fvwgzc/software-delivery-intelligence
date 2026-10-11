import { ChevronRightIcon } from 'lucide-react'
import { memo, type MouseEvent } from 'react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { cn } from '@/lib/utils'

import type { TreeNode } from '../../lib/tree'
import { EntryGlyph } from '../entry-glyph'
import { INDENT, ROW_CLASS } from './constants'

interface TreeRowProps {
  id: string
  node: TreeNode
  projectId: string
  expanded: boolean
  /** The path the page shows. */
  selected: boolean
  /** The row keyboard navigation is on. */
  active: boolean
  onToggle: (node: TreeNode) => void
  onActivate: (node: TreeNode) => void
}

/**
 * One tree item. The whole row navigates; the chevron only toggles. Rows
 * are not tab stops themselves: the tree container owns focus and points
 * here with `aria-activedescendant`.
 */
export const TreeRow = memo(function TreeRow({
  id,
  node,
  projectId,
  expanded,
  selected,
  active,
  onToggle,
  onActivate,
}: TreeRowProps) {
  const toggle = (event: MouseEvent) => {
    event.preventDefault()
    event.stopPropagation()
    onToggle(node)
  }

  return (
    <div
      id={id}
      role="treeitem"
      aria-level={node.depth + 1}
      aria-expanded={node.isDir ? expanded : undefined}
      aria-selected={selected}
      data-active={active || undefined}
      className={cn(
        ROW_CLASS,
        'group/row relative flex items-center text-sm transition-colors hover:bg-muted/60 group-focus-visible/tree:data-active:ring-1 group-focus-visible/tree:data-active:ring-inset group-focus-visible/tree:data-active:ring-ring/50',
        selected && 'bg-brand/8 text-foreground',
      )}
      style={{ paddingLeft: node.depth * INDENT + 4 }}
    >
      {node.isDir ? (
        <button
          type="button"
          tabIndex={-1}
          aria-label={expanded ? `Collapse ${node.name}` : `Expand ${node.name}`}
          onClick={toggle}
          className="flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:text-foreground"
        >
          <ChevronRightIcon
            className={cn('size-3.5 transition-transform duration-150', expanded && 'rotate-90')}
            aria-hidden="true"
          />
        </button>
      ) : (
        <span className="size-5 shrink-0" aria-hidden="true" />
      )}
      <Link
        to={routes.projectCode(projectId, node.path)}
        tabIndex={-1}
        onClick={() => onActivate(node)}
        className={cn(
          'flex min-w-0 flex-1 items-center gap-1.5 pr-2 outline-none',
          selected ? 'text-foreground' : 'text-foreground/85 hover:text-foreground',
        )}
      >
        <EntryGlyph
          name={node.name}
          isDir={node.isDir}
          language={node.language}
          open={expanded}
          className={cn('size-4 shrink-0', node.isDir ? 'text-foreground/70' : 'text-muted-foreground')}
        />
        <span className="truncate">{node.name}</span>
      </Link>
    </div>
  )
})
