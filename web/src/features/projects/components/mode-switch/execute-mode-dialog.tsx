import { ShieldAlertIcon } from 'lucide-react'

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

interface ExecuteModeDialogProps {
  projectName: string
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
  pending: boolean
}

/** Confirms the one setting that lets the platform act, not only observe. */
export function ExecuteModeDialog({ projectName, open, onOpenChange, onConfirm, pending }: ExecuteModeDialogProps) {
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogMedia className="bg-warning/10 text-warning">
            <ShieldAlertIcon />
          </AlertDialogMedia>
          <AlertDialogTitle>Enable execute mode for {projectName}?</AlertDialogTitle>
          <AlertDialogDescription>
            Accepted plans can then run against this project. Every step still passes permission, policy
            and approval checks, and observe mode can be restored at any time.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>Keep observing</AlertDialogCancel>
          <Button onClick={onConfirm} loading={pending}>
            Enable execute mode
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
