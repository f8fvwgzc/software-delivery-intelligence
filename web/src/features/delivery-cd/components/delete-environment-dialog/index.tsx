import { Trash2Icon } from 'lucide-react'
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
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'

import { useDeleteEnvironment } from '../../api/mutations'
import type { Environment } from '../../api/types'

interface DeleteEnvironmentDialogProps {
  projectId: string
  environment: Environment
  open: boolean
  onOpenChange: (open: boolean) => void
  onCloseAutoFocus?: (event: Event) => void
}

/** Removes a stage from the board. What runs at its URL is not touched. */
export function DeleteEnvironmentDialog({ projectId, environment, open, onOpenChange, onCloseAutoFocus }: DeleteEnvironmentDialogProps) {
  const remove = useDeleteEnvironment(projectId)

  const confirm = () => {
    remove.mutate(environment.id, {
      onSuccess: () => {
        onOpenChange(false)
        toast.success(`${environment.name} removed from the pipeline`)
      },
    })
  }

  return (
    <AlertDialog open={open} onOpenChange={(next) => (remove.isPending ? undefined : onOpenChange(next))}>
      <AlertDialogContent onCloseAutoFocus={onCloseAutoFocus}>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-destructive/10 text-destructive">
            <Trash2Icon />
          </AlertDialogMedia>
          <AlertDialogTitle>Delete {environment.name}?</AlertDialogTitle>
          <AlertDialogDescription>
            The stage leaves the board and the promotion order closes up behind it. Past deployments stay in the history, and
            whatever runs there now keeps running: nothing is undeployed.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={remove.isPending}>Keep environment</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={remove.isPending}>
            Delete environment
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
