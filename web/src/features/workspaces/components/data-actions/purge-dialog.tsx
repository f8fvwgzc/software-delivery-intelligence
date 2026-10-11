import { EraserIcon } from 'lucide-react'
import { useState } from 'react'

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

import { usePurgeCodeCache } from '../../api/mutations'

interface PurgeDialogProps {
  workspaceId: string
  disabled: boolean
}

/** Drops every cached file and tree of the workspace; the next code view refetches from GitHub. */
export function PurgeDialog({ workspaceId, disabled }: PurgeDialogProps) {
  const [open, setOpen] = useState(false)
  const purge = usePurgeCodeCache(workspaceId)

  const confirm = () => {
    purge.mutate(undefined, { onSuccess: () => setOpen(false) })
  }

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <AlertDialogTrigger asChild>
        <Button variant="outline" size="sm" disabled={disabled}>
          <EraserIcon data-icon="inline-start" />
          Purge code cache
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-warning/10 text-warning">
            <EraserIcon />
          </AlertDialogMedia>
          <AlertDialogTitle>Purge the code cache?</AlertDialogTitle>
          <AlertDialogDescription>
            Cached file contents and repository trees for every project in this workspace are deleted. Facts, graph and
            proposals stay. The code browser and diagrams read from GitHub again on next use. The purge is audited.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={purge.isPending}>Keep cache</AlertDialogCancel>
          <Button onClick={confirm} loading={purge.isPending}>
            Purge cache
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
