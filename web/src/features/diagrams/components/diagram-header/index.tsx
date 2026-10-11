import { RefreshCwIcon, Trash2Icon } from 'lucide-react'

import { RelativeTime, StatusBadge } from '@/components/shared/data'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import type { Diagram } from '../../api/queries'
import { kindMeta, shortSha, sourceMeta } from '../../lib/labels'
import { DeleteDiagramDialog } from '../delete-diagram-dialog'
import { EditableTitle } from './editable-title'
import { ExportMenu, type Exporter } from './export-menu'

export { DiagramHeaderSkeleton } from './skeleton'

interface DiagramHeaderProps {
  diagram: Diagram
  onRename: (title: string) => void
  renaming: boolean
  onRegenerate: () => void
  regenerating: boolean
  getExporter: () => Exporter | null
  onDeleted: () => void
}

/** Title (editable), the diagram's provenance, and the three actions: regenerate, export, delete. */
export function DiagramHeader({
  diagram,
  onRename,
  renaming,
  onRegenerate,
  regenerating,
  getExporter,
  onDeleted,
}: DiagramHeaderProps) {
  const kind = kindMeta(diagram.kind)
  const source = sourceMeta(diagram.source)
  const KindIcon = kind.icon

  return (
    <header className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div className="flex min-w-0 flex-col gap-1.5">
        <EditableTitle value={diagram.title} onSave={onRename} saving={renaming} />
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-2xs text-muted-foreground">
          <Badge variant="outline" className="gap-1 font-normal text-muted-foreground">
            <KindIcon data-icon="inline-start" />
            {kind.label}
          </Badge>
          <Tooltip>
            <TooltipTrigger asChild>
              <StatusBadge tone={source.tone}>{source.label}</StatusBadge>
            </TooltipTrigger>
            <TooltipContent>{source.description}</TooltipContent>
          </Tooltip>
          <span className="font-mono tabular-nums">r{diagram.revision}</span>
          {diagram.commitSha ? (
            <>
              <span aria-hidden="true">·</span>
              <span className="font-mono" title={diagram.commitSha}>
                {shortSha(diagram.commitSha)}
              </span>
            </>
          ) : null}
          <span aria-hidden="true">·</span>
          <span>
            {diagram.reasoner === 'claude' ? 'Reasoned' : 'Heuristic'}, updated{' '}
            <RelativeTime value={diagram.updatedAt} className="font-mono" />
          </span>
        </div>
      </div>

      <div className="flex shrink-0 flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={onRegenerate} loading={regenerating}>
          <RefreshCwIcon data-icon="inline-start" />
          Regenerate
        </Button>
        <ExportMenu title={diagram.title} getExporter={getExporter} />
        <DeleteDiagramDialog
          diagram={diagram}
          tooltip="Delete diagram"
          onDeleted={onDeleted}
          trigger={
            <Button variant="ghost" size="icon-sm" aria-label="Delete diagram" className="text-muted-foreground hover:text-destructive">
              <Trash2Icon />
            </Button>
          }
        />
      </div>
    </header>
  )
}
