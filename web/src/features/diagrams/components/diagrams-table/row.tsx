import { useQueryClient } from '@tanstack/react-query'
import { Trash2Icon } from 'lucide-react'
import { Link, useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { RelativeTime, StatusBadge } from '@/components/shared/data'
import { Button } from '@/components/ui/button'
import { TableCell, TableRow } from '@/components/ui/table'

import { diagramQuery, type Diagram } from '../../api/queries'
import { kindMeta, shortSha, sourceMeta } from '../../lib/labels'
import { DeleteDiagramDialog } from '../delete-diagram-dialog'

export function DiagramRow({ diagram }: { diagram: Diagram }) {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const kind = kindMeta(diagram.kind)
  const source = sourceMeta(diagram.source)
  const KindIcon = kind.icon
  const href = routes.diagram(diagram.id)

  const prefetch = () => {
    queryClient.prefetchQuery({ ...diagramQuery(diagram.id), staleTime: 30_000 })
  }

  return (
    // The row click is a pointer convenience; the title link is the real
    // navigation and the keyboard target, so the row stays out of the tab order.
    <TableRow className="cursor-pointer" onClick={() => navigate(href)} onMouseEnter={prefetch}>
      <TableCell className="max-w-80">
        <span className="flex min-w-0 items-center gap-2">
          <KindIcon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <Link
            to={href}
            className="truncate rounded-sm font-medium outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50"
            onClick={(event) => event.stopPropagation()}
            onFocus={prefetch}
          >
            {diagram.title}
          </Link>
        </span>
      </TableCell>
      <TableCell className="text-xs text-muted-foreground">{kind.label}</TableCell>
      <TableCell>
        <StatusBadge tone={source.tone}>{source.label}</StatusBadge>
      </TableCell>
      <TableCell className="font-mono text-xs tabular-nums text-muted-foreground">r{diagram.revision}</TableCell>
      <TableCell className="font-mono text-xs text-muted-foreground" title={diagram.commitSha}>
        {shortSha(diagram.commitSha) || '—'}
      </TableCell>
      <TableCell className="text-right text-xs text-muted-foreground">
        <RelativeTime value={diagram.updatedAt} className="font-mono tabular-nums" />
      </TableCell>
      <TableCell className="w-10 text-right" onClick={(event) => event.stopPropagation()}>
        <DeleteDiagramDialog
          diagram={diagram}
          tooltip="Delete diagram"
          trigger={
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={`Delete ${diagram.title}`}
              className="text-muted-foreground hover:text-destructive"
            >
              <Trash2Icon />
            </Button>
          }
        />
      </TableCell>
    </TableRow>
  )
}
