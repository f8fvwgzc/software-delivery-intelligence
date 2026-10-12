import { FolderIcon } from 'lucide-react'

import { cn } from '@/lib/utils'

import { FileGlyph } from './file-icon'
import { baseName, type FileGroup } from './files'

interface FileTreeProps {
  groups: FileGroup[]
  selectedPath: string
  onSelect: (path: string) => void
  /** Accessible name of the tree. */
  label: string
}

/** Files grouped by directory. One button per file; the selected one is marked and announced. */
export function FileTree({ groups, selectedPath, onSelect, label }: FileTreeProps) {
  return (
    <nav aria-label={label} className="flex flex-col gap-3 p-2">
      {groups.map((group) => (
        <div key={group.directory || '/'} className="flex flex-col gap-0.5">
          <div className="flex h-6 items-center gap-1.5 px-1.5 font-mono text-2xs text-muted-foreground">
            <FolderIcon className="size-3.5 shrink-0" aria-hidden="true" />
            <span className="truncate">{group.directory === '' ? '/' : `${group.directory}/`}</span>
          </div>
          <ul className="flex flex-col gap-0.5">
            {group.files.map((file) => {
              const selected = file.path === selectedPath
              return (
                <li key={file.path}>
                  <button
                    type="button"
                    onClick={() => onSelect(file.path)}
                    aria-current={selected ? 'true' : undefined}
                    title={file.purpose}
                    className={cn(
                      'flex h-7 w-full items-center gap-2 rounded-md px-1.5 pl-5 text-left font-mono text-xs outline-none transition-colors',
                      'hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50',
                      selected ? 'bg-muted text-foreground shadow-[inset_2px_0_0_0_var(--brand)]' : 'text-foreground/80',
                    )}
                  >
                    <FileGlyph file={file} className="size-3.5 shrink-0 text-muted-foreground" />
                    <span className="truncate">{baseName(file.path)}</span>
                    {file.replacesExisting ? (
                      <span className="ml-auto shrink-0 text-2xs text-warning" aria-label="replaces an existing file">
                        replaces
                      </span>
                    ) : null}
                  </button>
                </li>
              )
            })}
          </ul>
        </div>
      ))}
    </nav>
  )
}
