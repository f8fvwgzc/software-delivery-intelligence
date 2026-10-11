import { CornerLeftUpIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { formatBytes } from '../../lib/format'
import { parentPath } from '../../lib/path'
import type { TreeNode } from '../../lib/tree'
import { EntryGlyph } from '../entry-glyph'

export const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'
export const CELL_CLASS = {
  icon: 'h-10 w-8 pl-3 pr-0',
  name: 'h-10 px-3',
  size: 'h-10 w-24 px-3 text-right font-mono text-xs tabular-nums text-muted-foreground',
} as const

const LINK_CLASS =
  'inline-flex max-w-full items-center rounded-sm outline-none hover:underline hover:underline-offset-4 focus-visible:ring-2 focus-visible:ring-ring/50'

interface EntriesTableProps {
  projectId: string
  /** `''` at the repository root. */
  path: string
  entries: TreeNode[]
}

/** GitHub-style listing: icon, name, size. Directories first, as the tree sorts them. */
export function EntriesTable({ projectId, path, entries }: EntriesTableProps) {
  return (
    <Table>
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead className={HEAD_CLASS} colSpan={2}>
            Name
          </TableHead>
          <TableHead className={`${HEAD_CLASS} text-right`}>Size</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {path !== '' ? (
          <TableRow>
            <TableCell className={CELL_CLASS.icon}>
              <CornerLeftUpIcon className="size-4 text-muted-foreground" aria-hidden="true" />
            </TableCell>
            <TableCell className={CELL_CLASS.name}>
              <Link to={routes.projectCode(projectId, parentPath(path))} className={LINK_CLASS} aria-label="Parent directory">
                ..
              </Link>
            </TableCell>
            <TableCell className={CELL_CLASS.size} />
          </TableRow>
        ) : null}
        {entries.map((entry) => (
          <TableRow key={entry.path}>
            <TableCell className={CELL_CLASS.icon}>
              <EntryGlyph
                name={entry.name}
                isDir={entry.isDir}
                language={entry.language}
                className={entry.isDir ? 'size-4 text-foreground/70' : 'size-4 text-muted-foreground'}
              />
            </TableCell>
            <TableCell className={CELL_CLASS.name}>
              <Link to={routes.projectCode(projectId, entry.path)} className={LINK_CLASS}>
                <span className="truncate">{entry.name}</span>
              </Link>
            </TableCell>
            <TableCell className={CELL_CLASS.size}>{entry.isDir ? '' : formatBytes(entry.size)}</TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
