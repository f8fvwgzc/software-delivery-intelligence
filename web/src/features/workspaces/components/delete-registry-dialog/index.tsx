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
import { useDeleteRegistry } from '@/features/delivery-cd/api/mutations'
import type { Registry } from '@/features/delivery-cd/api/types'
import { registryPath } from '@/features/delivery-cd/lib/labels'

interface DeleteRegistryDialogProps {
  /** The registry to delete; it stays set while the dialog closes. */
  registry: Registry | undefined
  open: boolean
  onOpenChange: (open: boolean) => void
}

/** Removes a registry from the workspace. Images already pushed there are not touched. */
export function DeleteRegistryDialog({ registry, open, onOpenChange }: DeleteRegistryDialogProps) {
  const remove = useDeleteRegistry()

  const confirm = () => {
    if (!registry) return
    remove.mutate(registry.id, {
      onSuccess: () => {
        onOpenChange(false)
        toast.success(`${registry.name} removed`)
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
          <AlertDialogTitle>Delete {registry?.name}?</AlertDialogTitle>
          <AlertDialogDescription>
            Environments that name <span className="font-mono text-xs">{registry ? registryPath(registry) : ''}</span> fall back to
            the workspace default. Images already pushed there stay where they are.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={remove.isPending}>Keep registry</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={remove.isPending}>
            Delete registry
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
