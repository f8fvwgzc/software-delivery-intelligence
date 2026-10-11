import { Trash2Icon } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { toast } from 'sonner'

import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

import { useDeleteDiagram } from '../../api/mutations'
import type { Diagram } from '../../api/queries'

interface DeleteDiagramDialogProps {
  diagram: Pick<Diagram, 'id' | 'projectId' | 'title'>
  /** The button that opens the dialog. Must be a single element that takes a ref. */
  trigger: ReactNode
  /** Shown on hover of an icon-only trigger. */
  tooltip?: string
  onDeleted?: () => void
}

/** Confirms before a diagram is deleted. Its revisions go with it; the repository is untouched. */
export function DeleteDiagramDialog({ diagram, trigger, tooltip, onDeleted }: DeleteDiagramDialogProps) {
  const [open, setOpen] = useState(false)
  const remove = useDeleteDiagram(diagram.projectId)

  const confirm = () => {
    remove.mutate(diagram.id, {
      onSuccess: () => {
        setOpen(false)
        toast.success('Diagram deleted', { description: diagram.title })
        onDeleted?.()
      },
      onError: (error) => {
        toast.error('Could not delete the diagram', { description: error.message })
      },
    })
  }

  const dialogTrigger = <AlertDialogTrigger asChild>{trigger}</AlertDialogTrigger>

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      {tooltip ? (
        <Tooltip>
          <TooltipTrigger asChild>{dialogTrigger}</TooltipTrigger>
          <TooltipContent>{tooltip}</TooltipContent>
        </Tooltip>
      ) : (
        dialogTrigger
      )}
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-destructive/10 text-destructive">
            <Trash2Icon />
          </AlertDialogMedia>
          <AlertDialogTitle>Delete this diagram?</AlertDialogTitle>
          <AlertDialogDescription>
            <span className="font-medium text-foreground">{diagram.title}</span> and every edit to it are removed.
            It can be generated again from the repository at any time.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={remove.isPending}>Keep diagram</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={remove.isPending}>
            Delete diagram
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
