import { Trash2Icon } from 'lucide-react'
import { useEffect } from 'react'
import { toast } from 'sonner'

import { RefusalNote } from '@/components/shared/feedback/refusal-note'
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
import { count } from '@/lib/intl'

import { useDeleteCluster } from '../../api/mutations'
import type { Cluster } from '../../api/types'

interface DeleteClusterDialogProps {
  /** The cluster to delete; it stays set while the dialog closes. */
  cluster: Cluster | undefined
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Removes a cluster from the workspace. Nothing in the cluster is touched.
 * The service refuses while an application deploys to it (412); its
 * sentence is shown here, word for word.
 */
export function DeleteClusterDialog({ cluster, open, onOpenChange }: DeleteClusterDialogProps) {
  const remove = useDeleteCluster()
  const { reset } = remove

  // A refusal belongs to the cluster it was about: each opening starts clean.
  useEffect(() => {
    if (open) reset()
  }, [open, reset])

  const confirm = () => {
    if (!cluster) return
    remove.mutate(cluster.id, {
      onSuccess: () => {
        onOpenChange(false)
        toast.success(`${cluster.name} removed`)
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
          <AlertDialogTitle>Delete {cluster?.name}?</AlertDialogTitle>
          <AlertDialogDescription>
            SDI forgets the cluster and stops watching it. Nothing running in it is touched, and the secret{' '}
            <span className="font-mono text-xs">{cluster?.kubeconfigSecret}</span> stays in the workspace.
          </AlertDialogDescription>
        </AlertDialogHeader>
        {cluster && cluster.applicationCount > 0 ? (
          <p className="text-xs leading-5 text-muted-foreground">
            {count(cluster.applicationCount, 'application deploys', 'applications deploy')} to it. A cluster in use cannot be deleted:
            delete those applications or move them to another cluster first.
          </p>
        ) : null}
        {remove.isError ? <RefusalNote error={remove.error} /> : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={remove.isPending}>Keep cluster</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={remove.isPending}>
            Delete cluster
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
