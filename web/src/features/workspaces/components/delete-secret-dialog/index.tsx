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
import { useDeleteSecret } from '@/features/delivery-cd/api/mutations'

interface DeleteSecretDialogProps {
  workspaceId: string
  /** The secret to delete; it stays set while the dialog closes, so the title does not blank out. */
  name: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** Deletes a workspace secret. There is no way back: the value was never readable. */
export function DeleteSecretDialog({ workspaceId, name, open, onOpenChange }: DeleteSecretDialogProps) {
  const remove = useDeleteSecret(workspaceId)

  const confirm = () => {
    remove.mutate(name, {
      onSuccess: () => {
        onOpenChange(false)
        toast.success(`${name} deleted`)
      },
    })
  }

  return (
    <AlertDialog open={open} onOpenChange={(next) => (remove.isPending ? undefined : onOpenChange(next))}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-destructive/10 text-destructive">
            <Trash2Icon />
          </AlertDialogMedia>
          <AlertDialogTitle>
            Delete <span className="font-mono">{name}</span>?
          </AlertDialogTitle>
          <AlertDialogDescription>
            Deployers and registries that name this secret fail at their next run, until it is set again. The value cannot be
            recovered: it was stored encrypted and never shown.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={remove.isPending}>Keep secret</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={remove.isPending}>
            Delete secret
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
