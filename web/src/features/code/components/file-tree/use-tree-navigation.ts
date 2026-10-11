import { useMemo, useState, type KeyboardEvent } from 'react'

import { parentPath } from '../../lib/path'
import type { TreeNode, TreeRow } from '../../lib/tree'
import { ROW_HEIGHT } from './constants'

interface TreeNavigationArgs {
  rows: TreeRow[]
  currentPath: string
  onToggle: (node: TreeNode) => void
  onOpen: (node: TreeNode) => void
}

/** Keeps the row at `index` inside the scroll container's visible band. */
function revealRow(container: HTMLElement, index: number) {
  const top = index * ROW_HEIGHT
  const bottom = top + ROW_HEIGHT
  if (top < container.scrollTop) {
    container.scrollTop = top
  } else if (bottom > container.scrollTop + container.clientHeight) {
    container.scrollTop = bottom - container.clientHeight
  }
}

/**
 * Roving "active row" for the tree (the ARIA tree pattern with
 * `aria-activedescendant`): ↑/↓ move, → expands or steps into, ← collapses
 * or steps out, Enter opens. The active row falls back to the current path.
 */
export function useTreeNavigation({ rows, currentPath, onToggle, onOpen }: TreeNavigationArgs) {
  const [activePath, setActivePath] = useState<string | null>(null)

  const indexByPath = useMemo(() => new Map(rows.map((row, index) => [row.node.path, index])), [rows])

  const activeIndex = (activePath === null ? undefined : indexByPath.get(activePath)) ?? indexByPath.get(currentPath) ?? 0
  const activeRow: TreeRow | undefined = rows[activeIndex]

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (rows.length === 0 || !activeRow) return
    const container = event.currentTarget

    const moveTo = (index: number) => {
      const clamped = Math.max(0, Math.min(rows.length - 1, index))
      setActivePath(rows[clamped].node.path)
      revealRow(container, clamped)
    }

    switch (event.key) {
      case 'ArrowDown':
        moveTo(activeIndex + 1)
        break
      case 'ArrowUp':
        moveTo(activeIndex - 1)
        break
      case 'Home':
        moveTo(0)
        break
      case 'End':
        moveTo(rows.length - 1)
        break
      case 'ArrowRight':
        if (!activeRow.node.isDir) return
        if (activeRow.expanded) {
          if (activeRow.node.children.length > 0) moveTo(activeIndex + 1)
        } else {
          onToggle(activeRow.node)
        }
        break
      case 'ArrowLeft': {
        if (activeRow.node.isDir && activeRow.expanded) {
          onToggle(activeRow.node)
          break
        }
        const parentIndex = indexByPath.get(parentPath(activeRow.node.path))
        if (parentIndex !== undefined) moveTo(parentIndex)
        break
      }
      case 'Enter':
      case ' ':
        onOpen(activeRow.node)
        break
      default:
        return
    }
    event.preventDefault()
  }

  return { activeIndex, activePath: activeRow?.node.path ?? null, setActivePath, onKeyDown }
}
