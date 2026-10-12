import { useState } from 'react'

import { CodePane } from './code-pane'
import { FileTree } from './file-tree'
import { groupByDirectory, type ViewerFile } from './files'

export { FileGlyph } from './file-icon'
export { baseName, directoryOf, downloadFile, downloadFilesZip, fileIcon, groupByDirectory, splitLines, type FileGroup, type ViewerFile } from './files'
export { FileViewerSkeleton } from './skeleton'

interface FileViewerProps {
  /** At least one file; the caller renders its own empty state. */
  files: readonly ViewerFile[]
  /** Accessible name of the tree. */
  label?: string
}

/**
 * Generated files as a directory tree beside a read-only code pane. The
 * first file is open until another is chosen; a container query stacks the
 * tree above the pane when the region is narrow.
 */
export function FileViewer({ files, label = 'Generated files' }: FileViewerProps) {
  const groups = groupByDirectory(files)
  const [chosenPath, setChosenPath] = useState<string | null>(null)
  const selected = files.find((file) => file.path === chosenPath) ?? groups[0]?.files[0]

  if (!selected) return null

  return (
    // Container query: the tree sits beside the pane only when the region itself is wide enough.
    <div className="@container" data-slot="file-viewer">
      <div className="grid overflow-hidden rounded-lg ring-1 ring-border @2xl:grid-cols-[14rem_minmax(0,1fr)]">
        <div className="border-b @2xl:border-r @2xl:border-b-0">
          <FileTree groups={groups} selectedPath={selected.path} onSelect={setChosenPath} label={label} />
        </div>
        <CodePane key={selected.path} file={selected} />
      </div>
    </div>
  )
}
