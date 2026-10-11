import { FolderOpenIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'

import { isReadme } from '../../lib/path'
import type { TreeNode } from '../../lib/tree'
import { EntriesTable } from './entries-table'
import { ReadmePanel } from './readme-panel'

interface DirectoryViewProps {
  projectId: string
  /** `''` at the repository root. */
  path: string
  entries: TreeNode[]
}

/** A directory: its entries as a table, then its README when it has one. */
export function DirectoryView({ projectId, path, entries }: DirectoryViewProps) {
  const readme = entries.find((entry) => !entry.isDir && isReadme(entry.name))

  if (entries.length === 0) {
    return (
      <EmptyState
        icon={FolderOpenIcon}
        title="Empty directory"
        description="Nothing is recorded under this path at the scanned commit."
        className="m-4"
      />
    )
  }

  return (
    <div className="flex min-w-0 flex-col">
      <EntriesTable projectId={projectId} path={path} entries={entries} />
      {readme ? <ReadmePanel projectId={projectId} path={readme.path} /> : null}
    </div>
  )
}

export { DirectoryViewSkeleton } from './directory-skeleton'
