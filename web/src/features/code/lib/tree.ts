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

/**
 * The collator is correct but slow (a microsecond a call); a repository
 * with tens of thousands of entries sorts a million pairs. Siblings are
 * compared on a precomputed lower-case key with digit runs compared as
 * numbers, which is what `numeric: true` does for the names that occur in
 * repositories, and only ties fall through to the collator.
 */
function naturalCompare(a: string, b: string): number {
  let i = 0
  let j = 0
  while (i < a.length && j < b.length) {
    const ca = a.charCodeAt(i)
    const cb = b.charCodeAt(j)
    const da = ca >= 48 && ca <= 57
    const db = cb >= 48 && cb <= 57
    if (da && db) {
      let ea = i
      while (ea < a.length && a.charCodeAt(ea) >= 48 && a.charCodeAt(ea) <= 57) ea++
      let eb = j
      while (eb < b.length && b.charCodeAt(eb) >= 48 && b.charCodeAt(eb) <= 57) eb++
      const na = Number(a.slice(i, ea))
      const nb = Number(b.slice(j, eb))
      if (na !== nb) return na < nb ? -1 : 1
      i = ea
      j = eb
      continue
    }
    if (ca !== cb) return ca < cb ? -1 : 1
    i++
    j++
  }
  return a.length - i - (b.length - j)
}

interface Keyed {
  node: TreeNode
  key: string
}

function sortChildren(nodes: TreeNode[]) {
  if (nodes.length < 2) return
  const keyed: Keyed[] = nodes.map((node) => ({ node, key: node.name.toLowerCase() }))
  keyed.sort((a, b) => {
    if (a.node.isDir !== b.node.isDir) return a.node.isDir ? -1 : 1
    const fast = naturalCompare(a.key, b.key)
    return fast !== 0 ? fast : collator.compare(a.node.name, b.node.name)
  })
  for (let index = 0; index < keyed.length; index++) nodes[index] = keyed[index].node
}

function sortDeep(nodes: TreeNode[]) {
  sortChildren(nodes)
  for (const node of nodes) {
    if (node.children.length > 0) sortDeep(node.children)
  }
}

/** Number of `/` in the path: the depth without allocating the ancestor list. */
function depthOf(path: string): number {
  let depth = 0
  for (let index = 0; index < path.length; index++) {
    if (path.charCodeAt(index) === 47) depth++
  }
  return depth
}

interface TreeSource {
  entries: RepoTreeEntry[]
  commitSha: string
  truncated: boolean
}

/**
 * Nests the flat entry list, directories first and case-insensitive within
 * a level. Built for the worst case (GitHub's cap is 100 000 entries): one
 * pass to index, one to attach, no per-entry allocation beyond the node.
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
      depth: depthOf(path),
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
      depth: depthOf(entry.path),
      children: [],
    })
  }

  // Attach every node to its parent; directories GitHub left out (truncated trees) are created on the way.
  const roots: TreeNode[] = []
  for (const node of [...byPath.values()]) {
    const parent = parentPath(node.path)
    if (parent === '') {
      roots.push(node)
      continue
    }
    let dir = byPath.get(parent)
    if (!dir) {
      // Create the missing chain top-down so each level is attached once.
      for (const ancestor of ancestorPaths(node.path)) {
        if (byPath.has(ancestor)) continue
        const created = ensureDir(ancestor)
        const above = parentPath(ancestor)
        if (above === '') roots.push(created)
        else ensureDir(above).children.push(created)
      }
      dir = byPath.get(parent) as TreeNode
    }
    dir.children.push(node)
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
    for (const ancestor of ancestorPaths(node.path)) {
      if (keep.has(ancestor)) break
      keep.add(ancestor)
    }
  }
  return keep
}

/** The directories the URL keeps open: everything above the path, and the path itself when it is a directory. */
export function openByPath(model: TreeModel, path: string): Set<string> {
  const open = new Set(ancestorPaths(path))
  if (path && model.byPath.get(path)?.isDir) open.add(path)
  return open
}
