import { useState, type ReactNode } from 'react'
import { toast } from 'sonner'

import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Textarea } from '@/components/ui/textarea'

import { useCancelExecution } from '../../api/mutations'

interface CancelDialogProps {
  executionId: string
  executionNumber: number
  /** The element that opens the dialog. */
  children: ReactNode
  /** Title and copy for the stop-after-failure variant. */
  intent?: 'cancel' | 'stop'
}

const COPY = {
  cancel: {
    title: 'Cancel this execution?',
    description: 'Pending approvals expire and no further steps run. Checkpointed steps are kept for the record.',
    confirm: 'Cancel execution',
    defaultReason: 'Cancelled by operator',
  },
  stop: {
    title: 'Stop instead of remediating?',
    description: 'The execution ends as cancelled. Nothing is retried and dependents stay blocked.',
    confirm: 'Stop execution',
    defaultReason: 'Stopped after failure',
  },
} as const

/** Destructive confirmation with a reason, as `POST /executions/{id}/cancel` requires one. */
export function CancelDialog({ executionId, executionNumber, children, intent = 'cancel' }: CancelDialogProps) {
  const [open, setOpen] = useState(false)
  const [reason, setReason] = useState('')
  const cancel = useCancelExecution(executionId)
  const copy = COPY[intent]

  const confirm = () => {
    cancel.mutate(reason.trim() || copy.defaultReason, {
      onSuccess: () => {
        toast.success(`Execution #${executionNumber} cancelled`)
        setOpen(false)
      },
    })
  }

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <AlertDialogTrigger asChild>{children}</AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{copy.title}</AlertDialogTitle>
          <AlertDialogDescription>{copy.description}</AlertDialogDescription>
        </AlertDialogHeader>
        <Field>
          <FieldLabel htmlFor={`cancel-reason-${executionId}`}>Reason</FieldLabel>
          <Textarea
            id={`cancel-reason-${executionId}`}
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            placeholder={copy.defaultReason}
            rows={2}
          />
        </Field>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={cancel.isPending}>Keep running</AlertDialogCancel>
          <Button variant="destructive" onClick={confirm} loading={cancel.isPending}>
            {copy.confirm}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
