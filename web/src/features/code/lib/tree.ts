import type { RepoTreeEntry } from '../api/queries'
import { ancestorPaths, baseName, parentPath } from './path'

export interface TreeNode {
  path: string
  name: string
  isDir: boolean
  size: number
  language: string
  /** 0 for a top-level entry. */
  depth: number
  children: TreeNode[]
}

export interface TreeModel {
  roots: TreeNode[]
  byPath: Map<string, TreeNode>
  commitSha: string
  truncated: boolean
}

const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true })

/** Directories first, then case-insensitive by name. */
export function compareNodes(a: TreeNode, b: TreeNode): number {
  if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
  return collator.compare(a.name, b.name)
}

function sortDeep(nodes: TreeNode[]) {
  nodes.sort(compareNodes)
  for (const node of nodes) {
    if (node.children.length > 0) sortDeep(node.children)
  }
}

interface TreeSource {
  entries: RepoTreeEntry[]
  commitSha: string
  truncated: boolean
}

/**
 * Nests the flat entry list, directories first and case-insensitive within
 * a level.
 */
export function buildTree({ entries, commitSha, truncated }: TreeSource): TreeModel {
  const byPath = new Map<string, TreeNode>()

  const ensureDir = (path: string): TreeNode => {
    const existing = byPath.get(path)
    if (existing) return existing
    const node: TreeNode = {
      path,
      name: baseName(path),
      isDir: true,
      size: 0,
      language: '',
      depth: ancestorPaths(path).length,
      children: [],
    }
    byPath.set(path, node)
    return node
  }

  for (const entry of entries) {
    if (byPath.has(entry.path)) continue
    byPath.set(entry.path, {
      path: entry.path,
      name: entry.name,
      isDir: entry.isDir,
      size: entry.size,
      language: entry.language,
      depth: ancestorPaths(entry.path).length,
      children: [],
    })
  }

  // Directories GitHub left out (truncated trees) still get a node.
  for (const entry of entries) {
    for (const ancestor of ancestorPaths(entry.path)) ensureDir(ancestor)
  }

  const roots: TreeNode[] = []
  for (const node of byPath.values()) {
    const parent = parentPath(node.path)
    if (parent === '') {
      roots.push(node)
    } else {
      ensureDir(parent).children.push(node)
    }
  }

  sortDeep(roots)
  return { roots, byPath, commitSha, truncated }
}

export interface TreeRow {
  node: TreeNode
  expanded: boolean
}

/** Depth-first rows of the open part of the tree, optionally limited to `include`. */
export function flattenTree(
  roots: TreeNode[],
  isExpanded: (node: TreeNode) => boolean,
  include: ReadonlySet<string> | null,
): TreeRow[] {
  const rows: TreeRow[] = []
  const visit = (nodes: TreeNode[]) => {
    for (const node of nodes) {
      if (include && !include.has(node.path)) continue
      const expanded = node.isDir && isExpanded(node)
      rows.push({ node, expanded })
      if (expanded) visit(node.children)
    }
  }
  visit(roots)
  return rows
}

/** Paths whose full path contains the query, plus every directory above them. `null` when the query is blank. */
export function matchPaths(model: TreeModel, query: string): Set<string> | null {
  const needle = query.trim().toLowerCase()
  if (needle === '') return null
  const keep = new Set<string>()
  for (const node of model.byPath.values()) {
    if (!node.path.toLowerCase().includes(needle)) continue
    keep.add(node.path)
    for (const ancestor of ancestorPaths(node.path)) keep.add(ancestor)
  }
  return keep
}

/** The directories the URL keeps open: everything above the path, and the path itself when it is a directory. */
export function openByPath(model: TreeModel, path: string): Set<string> {
  const open = new Set(ancestorPaths(path))
  if (path && model.byPath.get(path)?.isDir) open.add(path)
  return open
}
