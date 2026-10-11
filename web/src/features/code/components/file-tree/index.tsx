import { FolderSearchIcon } from 'lucide-react'
import { useCallback, useDeferredValue, useId, useMemo, useState, type UIEvent } from 'react'
import { useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'

import { flattenTree, matchPaths, openByPath, type TreeModel, type TreeNode } from '../../lib/tree'
import { OVERSCAN, ROW_HEIGHT } from './constants'
import { TreeFilter } from './tree-filter'
import { TreeRow } from './tree-row'
import { useTreeNavigation } from './use-tree-navigation'

interface FileTreeProps {
  model: TreeModel
  projectId: string
  /** The path the page shows; its ancestors stay open. */
  currentPath: string
  className?: string
}

/** Directories the user opened or closed on top of what the URL dictates. Reset when the URL moves. */
interface Overrides {
  forPath: string
  opened: ReadonlySet<string>
  closed: ReadonlySet<string>
}

const NO_OVERRIDES = (path: string): Overrides => ({ forPath: path, opened: new Set(), closed: new Set() })

function rowId(prefix: string, path: string) {
  return `${prefix}${encodeURIComponent(path)}`
}

/**
 * The file tree: built once from the flat entry list, expanded from the URL,
 * filtered by path, keyboard navigable, and windowed so a repository with
 * thousands of entries renders only the rows on screen.
 */
export function FileTree({ model, projectId, currentPath, className }: FileTreeProps) {
  const navigate = useNavigate()
  const idPrefix = useId()

  const [query, setQuery] = useState('')
  const deferredQuery = useDeferredValue(query)

  const [overrides, setOverrides] = useState<Overrides>(() => NO_OVERRIDES(currentPath))
  if (overrides.forPath !== currentPath) {
    // Navigating re-opens the new path's ancestors; manual openings survive.
    setOverrides({ forPath: currentPath, opened: overrides.opened, closed: new Set() })
  }

  const [viewport, setViewport] = useState({ top: 0, height: 0 })

  const urlOpen = useMemo(() => openByPath(model, currentPath), [model, currentPath])
  const matches = useMemo(() => matchPaths(model, deferredQuery), [model, deferredQuery])

  const rows = useMemo(() => {
    const isExpanded = (node: TreeNode) => {
      if (matches) return true
      if (overrides.closed.has(node.path)) return false
      return overrides.opened.has(node.path) || urlOpen.has(node.path)
    }
    return flattenTree(model.roots, isExpanded, matches)
  }, [model, matches, overrides, urlOpen])

  const toggle = useCallback((node: TreeNode) => {
    setOverrides((previous) => {
      const opened = new Set(previous.opened)
      const closed = new Set(previous.closed)
      const expanded = !closed.has(node.path) && (opened.has(node.path) || urlOpen.has(node.path))
      if (expanded) {
        opened.delete(node.path)
        closed.add(node.path)
      } else {
        closed.delete(node.path)
        opened.add(node.path)
      }
      return { forPath: previous.forPath, opened, closed }
    })
  }, [urlOpen])

  const open = useCallback(
    (node: TreeNode) => navigate(routes.projectCode(projectId, node.path)),
    [navigate, projectId],
  )

  const { activeIndex, activePath, setActivePath, onKeyDown } = useTreeNavigation({
    rows,
    currentPath,
    onToggle: toggle,
    onOpen: open,
  })

  const activate = useCallback((node: TreeNode) => setActivePath(node.path), [setActivePath])

  // Measure the scroll container once it exists and whenever it resizes.
  const containerRef = useCallback((element: HTMLDivElement | null) => {
    if (!element) return
    const observer = new ResizeObserver(() => {
      setViewport((previous) => (previous.height === element.clientHeight ? previous : { ...previous, height: element.clientHeight }))
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const top = event.currentTarget.scrollTop
    setViewport((previous) => (previous.top === top ? previous : { ...previous, top }))
  }

  const start = Math.max(0, Math.floor(viewport.top / ROW_HEIGHT) - OVERSCAN)
  const end = Math.min(rows.length, Math.ceil((viewport.top + Math.max(viewport.height, ROW_HEIGHT * 20)) / ROW_HEIGHT) + OVERSCAN)
  const visible = rows.slice(start, end)

  return (
    <div className={className}>
      <TreeFilter value={query} onChange={setQuery} matches={matches ? rows.length : null} />
      {rows.length === 0 ? (
        <div className="flex flex-col items-center gap-2 py-10 text-center">
          <FolderSearchIcon className="size-4 text-muted-foreground" aria-hidden="true" />
          <p className="text-xs text-muted-foreground">No path contains “{deferredQuery.trim()}”.</p>
        </div>
      ) : (
        <div
          ref={containerRef}
          role="tree"
          aria-label="Repository files"
          aria-activedescendant={activePath === null ? undefined : rowId(idPrefix, activePath)}
          tabIndex={0}
          onKeyDown={onKeyDown}
          onScroll={onScroll}
          className="group/tree -mx-1 min-h-0 flex-1 overflow-y-auto rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          style={{ scrollbarGutter: 'stable' }}
        >
          <div style={{ paddingTop: start * ROW_HEIGHT, paddingBottom: (rows.length - end) * ROW_HEIGHT }}>
            {visible.map((row, offset) => (
              <TreeRow
                key={row.node.path}
                id={rowId(idPrefix, row.node.path)}
                node={row.node}
                projectId={projectId}
                expanded={row.expanded}
                selected={row.node.path === currentPath}
                active={start + offset === activeIndex}
                onToggle={toggle}
                onActivate={activate}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

export { FileTreeSkeleton } from './tree-skeleton'
